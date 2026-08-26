pub mod compile;
#[cfg(feature = "vm-stats")]
mod jit_coverage;

use crate::value::Value;
use crate::vm::function::RawVariadicInternalFunctionHandler;
use crate::vm::function::{
    BinaryLongRecursionPlan, CallPlan, CallStrategy, CleanupMode, ComposedScalarDoubleFunctionPlan,
    ComposedScalarDoubleOp, ComposedScalarLongFunctionPlan, ComposedScalarLongOp,
    ComposedTypedLongFunctionPlan, ComposedTypedLongOp, DirectInternalFunctionHandler, FrameLayout,
    FunctionCommon, FunctionType, HotStatus, InternalFunction, InternalFunctionHandler,
    LongPlanProperty, LongPlanSource, LongPropertyMethodPlan, LongPropertyOp, LongRecursiveBase,
    LongRecursiveCombine, LongRecursiveCondition, ObjectArrayEntry, ObjectArrayFunctionPlan,
    ObjectArrayLongCall, ObjectArrayLongOp, ObjectArraySource, ObjectLongConditionalAdjustment,
    ObjectLongFunctionPlan, ObjectLongIntDivArm, ObjectLongModuloAnySelect,
    ObjectLongModuloEqualTerm, ObjectLongObjectSource, ObjectLongOp, ObjectLongSource,
    ObjectLongStringAdjustment, ObjectLongStringIntDivCase, ObjectLongStringIntDivSelect,
    ObjectLongWeightedStringScore, ParamTypeHint, PropertyGetterMethodPlan, PropertyInitAssignment,
    PropertyInitMethodPlan, ReturnStrategy, ScalarDoubleCall, ScalarDoubleFunctionPlan,
    ScalarDoubleOp, ScalarDoubleOpKind, ScalarDoubleProgram, ScalarDoubleSelect,
    ScalarDoubleSource, ScalarLongCall, ScalarLongCallGuard, ScalarLongConditionKind,
    ScalarLongConditionOperand, ScalarLongFunctionPlan, ScalarLongOp, ScalarLongOpKind,
    ScalarLongProgram, ScalarLongSelect, ScalarLongSource, ScalarStringFunctionPlan,
    ScalarStringSelect, ScalarStringSource, SignatureInfo, UserFunction,
};
#[cfg(all(feature = "quick-loops", feature = "jit-prototype"))]
use crate::vm::function::{
    CapturedTypedLongFunctionPlan, IndirectScalarLongCallable, IndirectScalarLongFunctionPlan,
};
use crate::vm::instruction::{
    FETCH_DIM_FUNC_ARG, InlineCache, Instruction, KnownScalarType, LATE_STATIC_PROP_EMBEDDED_SCOPE,
    OpType, SEND_FLAG_YIELD_SNAPSHOT,
};
use crate::vm::opcode::OpCode;
use crate::vm::planner::{BlockInfo, BlockPlan};
use std::cell::Cell;
use std::collections::HashMap;

/// Compiled function body and its RPHP bytecode metadata.
pub struct OpArray {
    pub num_cvs: u32,
    pub num_temps: u32,
    /// Absolute frame slot initialized with the nearest final class that
    /// composed shared trait bytecode. Ordinary functions leave it empty.
    pub trait_class_scope_tmp: Option<u16>,
    pub instructions: Vec<Instruction>,
    /// Sorted sparse `(instruction index, source line)` metadata for opcodes
    /// whose location is observable. Kept out of `Instruction` so ordinary
    /// bytecode stays 16 B and unlocated instructions consume no side entry.
    pub source_lines: Vec<(u32, u32)>,
    pub literals: Vec<Value>,
    pub try_entries: Vec<compile::TryEntry>,
    /// Per-file strict_types flag, set by `declare(strict_types=1);`
    pub strict_types: bool,
    /// True if this function contains yield — it's a generator
    pub is_generator: bool,
    /// CVs bound to global variables via explicit `global $x;`: (cv_index, variable_name)
    pub global_vars: Vec<(u32, String)>,
    /// CVs bound to static variables: (cv_index, variable_name,
    /// compile-time-known initial value). Dynamic initializers have no value.
    pub static_vars: Vec<(u32, String, Option<Value>)>,
    /// Function name (for static variable storage key)
    pub name: String,
    /// Canonical source unit used by runtime diagnostics. Empty for synthetic
    /// bytecode assembled without source context.
    /// Shared source-unit name. Throwable origins clone this owner instead of
    /// allocating the same filename for every created exception.
    pub source_file: std::rc::Rc<String>,
    /// Main script scope CVs — all top-level variables synced to eg.globals before function calls.
    /// Empty for non-main-script op_arrays.
    pub main_scope_vars: Vec<(u32, String)>,
    /// All CVs in this op_array: (cv_index, variable_name).
    /// Used by include to share the caller's full local scope.
    pub all_cvs: Vec<(u32, String)>,
    /// Inline cache side table — one entry per instruction.
    /// Call initialization and generic guard opcodes use their own entries.
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

    /// Main/include units use their source filename as the op-array name;
    /// synthetic request roots use `<main>`. Function and method op-arrays
    /// carry their declared callable name instead.
    #[inline]
    pub fn is_main_script(&self) -> bool {
        self.name == "<main>" || (!self.source_file.is_empty() && self.name == *self.source_file)
    }

    #[inline]
    pub fn source_line(&self, instruction_index: usize) -> Option<usize> {
        self.source_lines
            .binary_search_by_key(&(instruction_index as u32), |(index, _)| *index)
            .ok()
            .map(|position| self.source_lines[position].1 as usize)
    }

    /// Cold declaration origin retained outside the instruction stream. The
    /// sentinel does not enlarge opcodes or the hot OpArray header.
    pub fn declaration_line(&self) -> Option<usize> {
        self.source_lines
            .last()
            .filter(|(index, _)| *index == u32::MAX)
            .map(|(_, line)| *line as usize)
    }

    /// Split by-value foreach writes after the complete function body is
    /// available. CVs that can become PHP references keep the canonical
    /// assignment-aware opcode; proven frame-local CVs use a branch-free
    /// opcode without weakening reference semantics.
    pub fn specialize_foreach_target_writes(
        &mut self,
        ref_args: u64,
        this_offset: u32,
        reference_cvs: &[u32],
    ) {
        let mut may_reference = vec![false; self.num_cvs as usize];
        for parameter in 0..64 {
            if ref_args & (1u64 << parameter) != 0 {
                let cv = this_offset + parameter;
                if let Some(slot) = may_reference.get_mut(cv as usize) {
                    *slot = true;
                }
            }
        }
        for &cv in reference_cvs {
            if let Some(slot) = may_reference.get_mut(cv as usize) {
                *slot = true;
            }
        }
        for &(cv, _) in &self.global_vars {
            if let Some(slot) = may_reference.get_mut(cv as usize) {
                *slot = true;
            }
        }
        for &(cv, _, _) in &self.static_vars {
            if let Some(slot) = may_reference.get_mut(cv as usize) {
                *slot = true;
            }
        }

        for instruction in &self.instructions {
            let mut mark = |cv: u16| {
                if let Some(slot) = may_reference.get_mut(cv as usize) {
                    *slot = true;
                }
            };
            match instruction.opcode {
                OpCode::BindGlobal | OpCode::CheckStatic | OpCode::BindStatic => {
                    mark(instruction.op1)
                }
                OpCode::SendVarEx | OpCode::SendNamed
                    if instruction._pad & SEND_FLAG_YIELD_SNAPSHOT != 0 =>
                {
                    mark(instruction.result)
                }
                OpCode::SendRef | OpCode::SendVarEx | OpCode::SendNamed
                    if instruction.op1_type == OpType::Cv =>
                {
                    mark(instruction.op1)
                }
                OpCode::BindArrayAppendRef
                | OpCode::BindObjPropRef
                | OpCode::BindArrayDimRef
                | OpCode::BindGlobalRef
                    if instruction.result_type == OpType::Cv =>
                {
                    mark(instruction.result)
                }
                OpCode::FetchDimR
                    if instruction._pad & FETCH_DIM_FUNC_ARG != 0
                        && instruction.result_type == OpType::Cv =>
                {
                    mark(instruction.result)
                }
                OpCode::AssignGlobalRef if instruction.op2_type == OpType::Cv => {
                    mark(instruction.op2)
                }
                OpCode::BindCvRef => {
                    mark(instruction.op1);
                    mark(instruction.result);
                }
                OpCode::BindDynamicVarRef
                | OpCode::AssignDynamicVarRef
                | OpCode::BindDynamicGlobal => may_reference.fill(true),
                OpCode::ClosureUseVar
                    if instruction.op2_type == OpType::Cv
                        && instruction._pad & crate::vm::instruction::CLOSURE_USE_REFERENCE
                            != 0 =>
                {
                    mark(instruction.op2)
                }
                OpCode::ForeachNextRef => mark(instruction.extended_value as u16),
                // Included code shares the current symbol table and may bind
                // any visible local by reference.
                OpCode::Include => may_reference.fill(true),
                _ => {}
            }
        }

        for instruction in &mut self.instructions {
            if matches!(
                instruction.opcode,
                OpCode::ForeachNext | OpCode::ForeachNextPlain
            ) {
                let value_cv = (instruction.extended_value & 0xffff) as usize;
                instruction.opcode = if may_reference.get(value_cv).copied().unwrap_or(true) {
                    OpCode::ForeachNext
                } else {
                    OpCode::ForeachNextPlain
                };
            }
        }
    }

    /// Rewrite Tmp/Var operand indices from relative (0-based tmp index) to
    /// absolute slot offset (num_cvs + tmp_index). After this pass, runtime
    /// can access Tmp slots as `frame_base.add(operand)` without loading num_cvs.
    pub fn resolve_tmp_offsets(&mut self) {
        use crate::vm::instruction::OpType;
        use crate::vm::opcode::OpCode;
        let offset = self.num_cvs;
        let offset16 = offset as u16;
        if let Some(scope_tmp) = &mut self.trait_class_scope_tmp {
            *scope_tmp += offset16;
        }
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
        if len < 2 {
            return;
        }
        let mut i = 0;
        while i < len - 1 {
            let curr = self.instructions[i];
            let next = self.instructions[i + 1];
            // Pattern A: comparison + conditional jump → fused branch
            let fused_cmp = match (curr.opcode, next.opcode) {
                (OpCode::IsSmallerOrEqual_CvConst, OpCode::JmpZ)
                    if next.op1_type == OpType::Tmp
                        && next.op1 == curr.result
                        && curr.result_type == OpType::Tmp =>
                {
                    Some(OpCode::JmpZ_Le_CvConst)
                }
                (OpCode::IsSmallerOrEqual_CvConst, OpCode::JmpNZ)
                    if next.op1_type == OpType::Tmp
                        && next.op1 == curr.result
                        && curr.result_type == OpType::Tmp =>
                {
                    Some(OpCode::JmpNZ_Le_CvConst)
                }
                (OpCode::IsSmaller_CvConst, OpCode::JmpZ)
                    if next.op1_type == OpType::Tmp
                        && next.op1 == curr.result
                        && curr.result_type == OpType::Tmp =>
                {
                    Some(OpCode::JmpZ_Lt_CvConst)
                }
                (OpCode::IsSmaller_CvConst, OpCode::JmpNZ)
                    if next.op1_type == OpType::Tmp
                        && next.op1 == curr.result
                        && curr.result_type == OpType::Tmp =>
                {
                    Some(OpCode::JmpNZ_Lt_CvConst)
                }
                (OpCode::IsEqual_CvConst, OpCode::JmpZ)
                    if next.op1_type == OpType::Tmp
                        && next.op1 == curr.result
                        && curr.result_type == OpType::Tmp =>
                {
                    Some(OpCode::JmpZ_Eq_CvConst)
                }
                (OpCode::IsEqual_CvConst, OpCode::JmpNZ)
                    if next.op1_type == OpType::Tmp
                        && next.op1 == curr.result
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
                    op1: curr.op1,    // CV index
                    op2: curr.op2,    // Const index
                    result: next.op2, // jump target IP
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
                OpCode::Jmp | OpCode::QuickLongLoopJmp | OpCode::AssertCheck => {
                    // Jmp stores target in op1
                    let target = instr.op1 as usize;
                    if target < n {
                        is_leader[target] = true;
                    }
                    if i + 1 < n {
                        is_leader[i + 1] = true;
                    }
                }
                OpCode::JmpFinally
                    if instr._pad & crate::vm::instruction::JMP_FLAG_FINALLY_END == 0 =>
                {
                    let target = instr.op1 as usize;
                    if target < n {
                        is_leader[target] = true;
                    }
                    if i + 1 < n {
                        is_leader[i + 1] = true;
                    }
                }
                OpCode::JmpFinally => {}
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
        // Establish caller-side escape facts independently of closed-loop
        // planning. Baseline execution uses these markers at the original
        // NewObj/InitMethodCall boundaries, while a typed loop may consume the
        // same complete constructor/result span as one operation. Keeping the
        // proof outside the feature gate makes the guarded call/return
        // aggregate path available to a baseline-only build as well.
        for init_ip in 0..self.instructions.len() {
            if crate::vm::quick::detect_object_array_consumer_span(self, init_ip).is_some() {
                self.instructions[init_ip]._pad |=
                    crate::vm::instruction::CALL_FLAG_OBJECT_ARRAY_CONSUMERS;
            }
        }
        for new_ip in 0..self.instructions.len() {
            if crate::vm::quick::detect_virtual_object_array_pipeline_span(self, new_ip).is_some() {
                self.instructions[new_ip]._pad |=
                    crate::vm::instruction::NEW_FLAG_VIRTUAL_OBJECT_ARRAY_PIPELINE;
            }
        }

        if !cfg!(feature = "quick-loops") {
            return;
        }
        if std::env::var_os("RPHP_DISABLE_QUICK_LOOPS").is_some() {
            return;
        }

        for new_ip in 0..self.instructions.len() {
            if crate::vm::quick::detect_virtual_declared_object_read_span(self, new_ip).is_some() {
                self.instructions[new_ip]._pad |=
                    crate::vm::instruction::NEW_FLAG_VIRTUAL_DECLARED_READS;
            }
        }
        for init_ip in 0..self.instructions.len() {
            if crate::vm::callback_pipeline::detect_callback_array_pipeline_span(self, init_ip)
                .is_some()
            {
                self.instructions[init_ip]._pad |=
                    crate::vm::instruction::CALL_FLAG_CALLBACK_ARRAY_PIPELINE;
            }
        }
        for init_ip in 0..self.instructions.len() {
            if crate::vm::callback_pipeline::detect_staged_callback_array_pipeline_span(
                self, init_ip,
            )
            .is_some()
            {
                self.instructions[init_ip]._pad |=
                    crate::vm::instruction::CALL_FLAG_STAGED_CALLBACK_ARRAY_PIPELINE;
            }
        }
        for init_ip in 0..self.instructions.len() {
            if let Some(span) =
                crate::vm::callback_pipeline::detect_filter_map_callback_array_pipeline_span(
                    self, init_ip,
                )
            {
                self.instructions[init_ip]._pad |=
                    crate::vm::instruction::CALL_FLAG_FILTER_MAP_CALLBACK_ARRAY_PIPELINE;
                if span.discarded_cvs.is_some() {
                    self.instructions[init_ip]._pad |=
                        crate::vm::instruction::CALL_FLAG_CALLBACK_ARRAY_PIPELINE_STAGED_METADATA;
                }
            }
        }
        for init_ip in 0..self.instructions.len() {
            if let Some(span) =
                crate::vm::callback_pipeline::detect_json_callback_array_pipeline_span(
                    self, init_ip,
                )
            {
                self.instructions[init_ip]._pad |=
                    crate::vm::instruction::CALL_FLAG_CALLBACK_ARRAY_PIPELINE_JSON_SINK;
                if span.pipeline.order
                    == crate::vm::callback_pipeline::CallbackArrayPipelineOrder::FilterMap
                {
                    self.instructions[init_ip]._pad |=
                        crate::vm::instruction::CALL_FLAG_CALLBACK_ARRAY_PIPELINE_FILTER_FIRST;
                }
                if span.pipeline.discarded_cvs.is_some() {
                    self.instructions[init_ip]._pad |=
                        crate::vm::instruction::CALL_FLAG_CALLBACK_ARRAY_PIPELINE_STAGED_METADATA;
                }
            }
        }

        let mut candidates = Vec::new();
        let mut closed_region_ip = vec![false; self.instructions.len()];

        for backedge_ip in 0..self.instructions.len() {
            let backedge = self.instructions[backedge_ip];
            if backedge.opcode != OpCode::Jmp {
                continue;
            }
            let header_ip = backedge.op1 as usize;
            if header_ip >= backedge_ip {
                continue;
            }
            #[cfg(feature = "vm-stats")]
            crate::vm::stats::inc_jit_loop_candidate();
            let plan = crate::vm::quick::detect_long_induction_loop(self, header_ip, backedge_ip)
                .map(BlockPlan::QuickLongInduction)
                .or_else(|| {
                    crate::vm::quick::detect_double_call_accumulate_loop(
                        self,
                        header_ip,
                        backedge_ip,
                    )
                    .map(BlockPlan::QuickDoubleCallAccumulate)
                })
                .or_else(|| {
                    crate::vm::quick::detect_long_accumulate_loop(self, header_ip, backedge_ip)
                        .map(BlockPlan::QuickLongAccumulate)
                })
                .or_else(|| {
                    crate::vm::quick::detect_foreach_object_property_accumulate_loop(
                        self,
                        header_ip,
                        backedge_ip,
                    )
                    .map(BlockPlan::QuickForeachObjectPropertyAccumulate)
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
                    #[cfg(feature = "vm-stats")]
                    {
                        let kind = match &plan {
                            BlockPlan::QuickLongInduction(_) => {
                                crate::vm::stats::JitRegionKind::LongInduction
                            }
                            BlockPlan::QuickDoubleCallAccumulate(_) => {
                                crate::vm::stats::JitRegionKind::DoubleCallAccumulate
                            }
                            BlockPlan::QuickLongAccumulate(_) => {
                                crate::vm::stats::JitRegionKind::LongAccumulate
                            }
                            BlockPlan::QuickForeachLongAccumulate(_) => {
                                crate::vm::stats::JitRegionKind::ForeachLongAccumulate
                            }
                            BlockPlan::QuickForeachObjectPropertyAccumulate(_) => {
                                crate::vm::stats::JitRegionKind::ForeachObjectPropertyAccumulate
                            }
                            BlockPlan::QuickLongOps(_) => {
                                crate::vm::stats::JitRegionKind::TypedOpsLoop
                            }
                            _ => unreachable!("quick-loop detector returned a non-loop plan"),
                        };
                        crate::vm::stats::inc_jit_loop_admitted(kind);
                    }
                    closed_region_ip[header_ip..=backedge_ip].fill(true);
                    candidates.push((backedge_ip, block_idx, plan));
                    continue;
                }
            }

            #[cfg(feature = "vm-stats")]
            {
                let reason = jit_coverage::loop_miss_reason(self, header_ip, backedge_ip);
                crate::vm::stats::inc_jit_loop_rejected(reason);
                if crate::vm::stats::enabled() {
                    self.instructions[backedge_ip].extended_value = reason.marker();
                }
            }
        }

        for (backedge_ip, block_idx, plan) in candidates {
            self.block_plans[block_idx as usize] = plan;
            self.instructions[backedge_ip].opcode = OpCode::QuickLongLoopJmp;
            self.instructions[backedge_ip].extended_value = block_idx as u32 + 1;
        }

        // Select a first straight-line application-region slice. Calls and
        // other semantic events stay in baseline; a subsequent array-result
        // extraction can reuse the same typed operation graph and exact side
        // exits as closed loops. `FetchDimR::extended_value` is otherwise
        // unused and stores the owning block plan index + 1.
        const MAX_STRAIGHT_REGION_INSTRUCTIONS: usize = 32;
        for entry_ip in 0..self.instructions.len() {
            let entry = self.instructions[entry_ip];
            if closed_region_ip[entry_ip]
                || entry.opcode != OpCode::FetchDimR
                || entry.extended_value != 0
            {
                continue;
            }
            let block_idx = *self.ip_to_block.get(entry_ip).unwrap_or(&u16::MAX);
            if block_idx == u16::MAX
                || !matches!(
                    self.block_plans.get(block_idx as usize),
                    Some(BlockPlan::Interpret)
                )
            {
                continue;
            }
            let block_end = self.block_info[block_idx as usize].end_ip as usize;
            let last_ip =
                block_end.min(entry_ip.saturating_add(MAX_STRAIGHT_REGION_INSTRUCTIONS - 1));
            if last_ip <= entry_ip {
                continue;
            }
            #[cfg(feature = "vm-stats")]
            crate::vm::stats::inc_jit_straight_candidate();

            #[cfg(feature = "vm-stats")]
            let mut saw_typed_span = false;
            #[cfg(feature = "vm-stats")]
            let mut admitted = false;
            for end_ip in (entry_ip + 1..=last_ip).rev() {
                if closed_region_ip[entry_ip..=end_ip]
                    .iter()
                    .any(|covered| *covered)
                {
                    continue;
                }
                let Some(plan) = crate::vm::quick::detect_long_ops_region(self, entry_ip, end_ip)
                else {
                    continue;
                };
                #[cfg(feature = "vm-stats")]
                {
                    saw_typed_span = true;
                }
                // Short straight-line regions must have a preselected dense
                // execution shape. Falling back to generic typed-op dispatch
                // here costs more than the baseline instructions it replaces.
                if plan.straight_array_kernel.is_none() {
                    continue;
                }
                self.block_plans[block_idx as usize] = BlockPlan::QuickLongOps(plan);
                self.instructions[entry_ip].extended_value = u32::from(block_idx) + 1;
                #[cfg(feature = "vm-stats")]
                {
                    crate::vm::stats::inc_jit_straight_admitted();
                    admitted = true;
                }
                break;
            }
            #[cfg(feature = "vm-stats")]
            {
                if !admitted {
                    let reason = if saw_typed_span {
                        crate::vm::stats::JitStraightMissReason::NoDenseKernel
                    } else {
                        crate::vm::stats::JitStraightMissReason::NoTypedSpan
                    };
                    crate::vm::stats::inc_jit_straight_rejected(reason);
                }
            }
        }
    }
}

impl Drop for OpArray {
    fn drop(&mut self) {
        // A DoFcall cache may retain an Rc-backed callback-name string. Pair
        // that reference here; all other cache kinds own no heap allocation.
        for (instruction, cache) in self.instructions.iter().zip(&self.cache) {
            if matches!(
                instruction.opcode,
                OpCode::DoFcall | OpCode::CallUserFuncArray | OpCode::InitUserCall
            ) {
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
        if matches!(
            instr.opcode,
            OpCode::DirectInternalCall1 | OpCode::DirectInternalCall2
        ) {
            let Some(kind) =
                crate::builtin_metadata::DirectInternalKind::from_id(instr.extended_value)
            else {
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
                | OpCode::BitwiseAnd_LongLong
                | OpCode::BitwiseOr
                | OpCode::BitwiseOr_LongLong
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
                | OpCode::InitLateStaticCall
                | OpCode::FetchLateStaticProp
                | OpCode::AssignStaticProp
                | OpCode::AssignLateStaticProp
                | OpCode::InitDynamicCall
                | OpCode::SendVal
                | OpCode::SendRef
                | OpCode::SendVarEx
                | OpCode::SendNamed
                | OpCode::CallUserFuncArray
                | OpCode::InitUserCall
                | OpCode::SendUser
                | OpCode::SendUserChecked
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

#[inline]
fn typed_function_supports_fast_return(
    op_array: &OpArray,
    return_type_hint: &ParamTypeHint,
) -> bool {
    op_array.global_vars.is_empty()
        && op_array.static_vars.is_empty()
        && op_array.try_entries.is_empty()
        && !op_array.is_generator
        && matches!(
            return_type_hint,
            ParamTypeHint::None
                | ParamTypeHint::Int
                | ParamTypeHint::Float
                | ParamTypeHint::String
                | ParamTypeHint::Bool
                | ParamTypeHint::Array
                | ParamTypeHint::Mixed
        )
}

#[inline]
fn mark_embedded_late_static_properties(op_array: &mut OpArray, embedded: bool) {
    if !embedded {
        return;
    }
    for instruction in &mut op_array.instructions {
        if matches!(
            instruction.opcode,
            OpCode::FetchLateStaticProp
                | OpCode::AssignLateStaticProp
                | OpCode::FetchLateClassConst
                | OpCode::FetchLateDynamicClassConst
        ) && instruction.op1_type == OpType::Const
            && op_array
                .literals
                .get(instruction.op1 as usize)
                .and_then(Value::as_str)
                .is_some_and(|class| {
                    class.eq_ignore_ascii_case("static") || class.eq_ignore_ascii_case("self")
                })
        {
            instruction._pad |= LATE_STATIC_PROP_EMBEDDED_SCOPE;
        }
    }
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
pub fn make_user_function_with_defaults(
    op_array: OpArray,
    num_args: u32,
    required_num_args: u32,
    is_variadic: bool,
) -> UserFunction {
    make_user_function_full(op_array, num_args, required_num_args, is_variadic, 0, 0)
}

/// Full constructor with all options.
pub fn make_user_function_full(
    mut op_array: OpArray,
    num_args: u32,
    required_num_args: u32,
    is_variadic: bool,
    variadic_cv_index: u32,
    ref_args: u64,
) -> UserFunction {
    let needs_trait_class_scope = op_array.trait_class_scope_tmp.is_some();
    op_array.specialize_foreach_target_writes(ref_args, 0, &[]);
    op_array.resolve_tmp_offsets();
    op_array.specialize_opcodes();
    if op_array.cache.len() != op_array.instructions.len() {
        op_array.init_cache();
    }
    op_array.compute_blocks();
    op_array.prepare_quick_loops();
    let needs_late_static_scope = op_array.instructions.iter().any(|instruction| {
        matches!(
            instruction.opcode,
            OpCode::InitLateStaticCall
                | OpCode::FetchLateStaticProp
                | OpCode::AssignLateStaticProp
                | OpCode::FetchLateClassConst
                | OpCode::FetchLateDynamicClassConst
        )
    });
    let is_fast_scalar = !is_variadic
        && !op_array.is_generator
        && ref_args == 0
        && num_args == required_num_args
        && op_array.global_vars.is_empty()
        && op_array.static_vars.is_empty()
        && op_array.try_entries.is_empty()
        && !op_array.may_access_globals;
    let call = if needs_late_static_scope {
        CallStrategy::Full
    } else if is_fast_scalar {
        CallStrategy::FastScalar
    } else if !is_variadic && !op_array.is_generator {
        CallStrategy::Fast
    } else {
        CallStrategy::Full
    };
    let cleanup = if op_array_supports_cleanup_fast(&op_array) {
        CleanupMode::SkipScan
    } else {
        CleanupMode::ScanAll
    };
    let ret = if !needs_late_static_scope
        && op_array.global_vars.is_empty()
        && op_array.static_vars.is_empty()
        && op_array.try_entries.is_empty()
        && !op_array.is_generator
    {
        ReturnStrategy::Fast
    } else {
        ReturnStrategy::Full
    };
    let num_cvs = op_array.num_cvs;
    let num_temps = op_array.num_temps;
    let has_reference_foreach = op_array
        .instructions
        .iter()
        .any(|instruction| instruction.opcode == OpCode::ForeachNextRef);
    let has_embedded_late_static_scope = needs_late_static_scope && num_cvs + num_temps <= 32;
    mark_embedded_late_static_properties(&mut op_array, has_embedded_late_static_scope);
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
                prefer_ref_args: 0,
                returns_reference: false,
                this_offset: 0,
                param_type_hints: vec![],
                param_names: vec![],
                return_type_hint: ParamTypeHint::None,
            },
            frame: FrameLayout {
                num_cvs,
                num_temps,
                total_slots,
            },
            plan: CallPlan::without_flags(call, ret, cleanup),
            call_count: Cell::new(0),
            hot_status: Cell::new(HotStatus::Cold),
        },
        op_array,
        attributes: Vec::new(),
        parameter_attributes: Vec::new(),
        parameter_default_diagnostics: None,
        reference_cvs: Vec::new(),
        long_property_plan: None,
        property_getter_plan: None,
        property_init_plan: None,
        binary_long_recursion_plan: None,
        scalar_long_plan: None,
        scalar_double_plan: None,
        composed_scalar_double_plan: None,
        object_long_plan: None,
        object_array_plan: None,
        scalar_string_plan: None,
        composed_scalar_long_plan: None,
        composed_typed_long_plan: None,
        compact_class_guard: Cell::new(0),
        borrowable_heap_args: 0,
        trait_class_scope_cache: needs_trait_class_scope
            .then(|| Box::new(crate::vm::function::TraitClassScopeCache::empty())),
    };
    function
        .common
        .plan
        .set_needs_late_static_scope(needs_late_static_scope);
    function
        .common
        .plan
        .set_has_embedded_late_static_scope(has_embedded_late_static_scope);
    function
        .common
        .plan
        .set_needs_trait_class_scope(needs_trait_class_scope);
    function
        .common
        .plan
        .set_has_reference_foreach(has_reference_foreach);
    let self_name = function.op_array.name.clone();
    function.binary_long_recursion_plan = build_binary_long_recursion_plan(&function, &self_name);
    function.scalar_long_plan = build_scalar_long_function_plan(&function);
    function.scalar_double_plan = build_scalar_double_function_plan(&function);
    function.composed_scalar_double_plan = build_composed_scalar_double_function_plan(&function);
    function.object_long_plan = build_object_long_function_plan(&function);
    function.object_array_plan = build_object_array_function_plan(&function);
    function.scalar_string_plan = build_scalar_string_function_plan(&function);
    function.composed_scalar_long_plan = build_composed_scalar_long_function_plan(&function);
    function.composed_typed_long_plan = build_composed_typed_long_function_plan(&function);
    #[cfg(all(feature = "quick-loops", feature = "jit-prototype"))]
    {
        let indirect_scalar_long_plan = build_indirect_scalar_long_function_plan(&function);
        function.set_indirect_scalar_long_plan(indirect_scalar_long_plan);
    }
    function.borrowable_heap_args = build_borrowable_heap_args(&function);
    function
}

/// Extended full constructor with type hints and param names.
pub fn make_user_function_typed(
    op_array: OpArray,
    num_args: u32,
    required_num_args: u32,
    is_variadic: bool,
    variadic_cv_index: u32,
    ref_args: u64,
    param_type_hints: Vec<ParamTypeHint>,
    param_names: Vec<String>,
    return_type_hint: ParamTypeHint,
) -> UserFunction {
    make_user_function_typed_with_return_mode(
        op_array,
        num_args,
        required_num_args,
        is_variadic,
        variadic_cv_index,
        ref_args,
        param_type_hints,
        param_names,
        return_type_hint,
        false,
    )
}

/// Parser-facing constructor which publishes the return mode before any
/// frame-free call plan is derived. The public constructor retains its
/// historical value-returning signature for embedders.
pub(crate) fn make_user_function_typed_with_return_mode(
    mut op_array: OpArray,
    num_args: u32,
    required_num_args: u32,
    is_variadic: bool,
    variadic_cv_index: u32,
    ref_args: u64,
    param_type_hints: Vec<ParamTypeHint>,
    param_names: Vec<String>,
    return_type_hint: ParamTypeHint,
    returns_reference: bool,
) -> UserFunction {
    let needs_trait_class_scope = op_array.trait_class_scope_tmp.is_some();
    op_array.specialize_foreach_target_writes(ref_args, 0, &[]);
    op_array.resolve_tmp_offsets();
    op_array.specialize_opcodes_with_hints(&param_type_hints);
    if op_array.cache.len() != op_array.instructions.len() {
        op_array.init_cache();
    }
    op_array.compute_blocks();
    op_array.prepare_quick_loops();
    // Exact all-`int` parameters use a distinct typed scalar ABI. Keeping it
    // separate leaves the original untyped FastScalar machine path untouched.
    let has_only_compact_hints = param_type_hints.iter().all(|h| {
        matches!(
            h,
            ParamTypeHint::None
                | ParamTypeHint::Int
                | ParamTypeHint::Float
                | ParamTypeHint::String
                | ParamTypeHint::Bool
                | ParamTypeHint::Array
                | ParamTypeHint::ClassName(_)
                | ParamTypeHint::Mixed
        )
    });
    let has_no_type_hints = param_type_hints
        .iter()
        .all(|hint| matches!(hint, ParamTypeHint::None | ParamTypeHint::Mixed));
    // `mixed` accepts every explicit value, including null, but PHP still
    // rejects a missing return value. Only an absent declaration is untyped.
    let has_no_return_type = matches!(return_type_hint, ParamTypeHint::None);
    let has_exact_long_params = !param_type_hints.is_empty()
        && param_type_hints
            .iter()
            .all(|hint| matches!(hint, ParamTypeHint::Int));
    let has_exact_long_return = matches!(
        return_type_hint,
        ParamTypeHint::None | ParamTypeHint::Mixed | ParamTypeHint::Int
    );
    let needs_late_static_scope = return_type_hint.uses_late_static()
        || op_array.instructions.iter().any(|instruction| {
            matches!(
                instruction.opcode,
                OpCode::InitLateStaticCall
                    | OpCode::FetchLateStaticProp
                    | OpCode::AssignLateStaticProp
                    | OpCode::FetchLateClassConst
                    | OpCode::FetchLateDynamicClassConst
            )
        });
    let has_fast_scalar_shape = !is_variadic
        && !op_array.is_generator
        && ref_args == 0
        && num_args == required_num_args
        && op_array.global_vars.is_empty()
        && op_array.static_vars.is_empty()
        && op_array.try_entries.is_empty()
        && !op_array.may_access_globals;
    let call = if needs_late_static_scope {
        // Late-static scope is recovered lazily in the already-cold full call
        // boundary. Ordinary static calls retain their exact compact path.
        CallStrategy::Full
    } else if has_fast_scalar_shape && has_no_type_hints && has_no_return_type {
        CallStrategy::FastScalar
    } else if has_fast_scalar_shape && has_exact_long_params && has_exact_long_return {
        CallStrategy::FastTypedScalar
    } else if !is_variadic && !op_array.is_generator && has_only_compact_hints {
        CallStrategy::Fast
    } else {
        CallStrategy::Full
    };
    let cleanup = if op_array_supports_cleanup_fast(&op_array) {
        CleanupMode::SkipScan
    } else {
        CleanupMode::ScanAll
    };
    let ret = if !returns_reference
        && !needs_late_static_scope
        && typed_function_supports_fast_return(&op_array, &return_type_hint)
    {
        ReturnStrategy::Fast
    } else {
        ReturnStrategy::Full
    };
    let num_cvs = op_array.num_cvs;
    let num_temps = op_array.num_temps;
    let has_reference_foreach = op_array
        .instructions
        .iter()
        .any(|instruction| instruction.opcode == OpCode::ForeachNextRef);
    let has_embedded_late_static_scope = needs_late_static_scope && num_cvs + num_temps <= 32;
    mark_embedded_late_static_properties(&mut op_array, has_embedded_late_static_scope);
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
                prefer_ref_args: 0,
                returns_reference,
                this_offset: 0,
                param_type_hints,
                param_names,
                return_type_hint,
            },
            frame: FrameLayout {
                num_cvs,
                num_temps,
                total_slots,
            },
            plan: CallPlan::without_flags(call, ret, cleanup),
            call_count: Cell::new(0),
            hot_status: Cell::new(HotStatus::Cold),
        },
        op_array,
        attributes: Vec::new(),
        parameter_attributes: Vec::new(),
        parameter_default_diagnostics: None,
        reference_cvs: Vec::new(),
        long_property_plan: None,
        property_getter_plan: None,
        property_init_plan: None,
        binary_long_recursion_plan: None,
        scalar_long_plan: None,
        scalar_double_plan: None,
        composed_scalar_double_plan: None,
        object_long_plan: None,
        object_array_plan: None,
        scalar_string_plan: None,
        composed_scalar_long_plan: None,
        composed_typed_long_plan: None,
        compact_class_guard: Cell::new(0),
        borrowable_heap_args: 0,
        trait_class_scope_cache: needs_trait_class_scope
            .then(|| Box::new(crate::vm::function::TraitClassScopeCache::empty())),
    };
    function
        .common
        .plan
        .set_needs_late_static_scope(needs_late_static_scope);
    function
        .common
        .plan
        .set_has_embedded_late_static_scope(has_embedded_late_static_scope);
    function
        .common
        .plan
        .set_needs_trait_class_scope(needs_trait_class_scope);
    function
        .common
        .plan
        .set_has_reference_foreach(has_reference_foreach);
    let self_name = function.op_array.name.clone();
    function.binary_long_recursion_plan = build_binary_long_recursion_plan(&function, &self_name);
    function.scalar_long_plan = build_scalar_long_function_plan(&function);
    function.scalar_double_plan = build_scalar_double_function_plan(&function);
    function.composed_scalar_double_plan = build_composed_scalar_double_function_plan(&function);
    function.object_long_plan = build_object_long_function_plan(&function);
    function.object_array_plan = build_object_array_function_plan(&function);
    function.scalar_string_plan = build_scalar_string_function_plan(&function);
    function.composed_scalar_long_plan = build_composed_scalar_long_function_plan(&function);
    function.composed_typed_long_plan = build_composed_typed_long_function_plan(&function);
    #[cfg(all(feature = "quick-loops", feature = "jit-prototype"))]
    {
        let indirect_scalar_long_plan = build_indirect_scalar_long_function_plan(&function);
        function.set_indirect_scalar_long_plan(indirect_scalar_long_plan);
    }
    function.borrowable_heap_args = build_borrowable_heap_args(&function);
    function
}

/// Prove which ordinary heap parameters can use the same synchronous borrowed
/// ABI as `$this`. Rebinding and String/Array COW mutation stay canonical;
/// object property mutation remains eligible because PHP shares object identity.
fn build_borrowable_heap_args(function: &UserFunction) -> u64 {
    let common = &function.common;
    let public_args = common.sig.public_arity().min(64);
    if function.op_array.is_generator
        || function.op_array.num_cvs + function.op_array.num_temps > 64
        || !function.op_array.try_entries.is_empty()
    {
        return 0;
    }

    let mut mask = if public_args == 64 {
        u64::MAX
    } else {
        (1u64 << public_args) - 1
    };
    mask &= !common.sig.ref_args;

    let clear_cv = |mask: &mut u64, cv: u16| {
        let cv = cv as u32;
        if cv >= common.sig.this_offset && cv < common.sig.this_offset + public_args {
            *mask &= !(1u64 << (cv - common.sig.this_offset));
        }
    };

    for instruction in &function.op_array.instructions {
        match instruction.opcode {
            // These opcodes may overwrite/drop the parameter variable itself.
            // Property mutation is intentionally absent: PHP objects have
            // shared identity, so `$arg->field = ...` is safe and observable.
            OpCode::AssignCv
            | OpCode::AssignConcat
            | OpCode::PreInc
            | OpCode::PreDec
            | OpCode::PostInc
            | OpCode::PostDec
            | OpCode::BindDefaultParam
            | OpCode::BindGlobal
            | OpCode::CheckStatic
            | OpCode::BindStatic => clear_cv(&mut mask, instruction.op1),
            // A local `=&` may expose either participating parameter through
            // the other CV for the rest of this frame.
            OpCode::BindCvRef => return 0,
            // In-place array operations require an owned Rc so make_mut can
            // observe the caller and detach according to PHP COW semantics.
            OpCode::AddArrayElement
            | OpCode::AddArrayUnpack
            | OpCode::AddCallArgument
            | OpCode::AddCallUnpack
            | OpCode::AssignDim
            | OpCode::ArrayPushOp
            | OpCode::UnsetDim
                if instruction.op1_type == OpType::Cv =>
            {
                clear_cv(&mut mask, instruction.op1)
            }
            // Foreach target placement is intentionally conservative until
            // its destination CV is explicit in the ownership analysis.
            OpCode::ForeachNext | OpCode::ForeachNextPlain => return 0,
            // A runtime-selected symbol-table name may designate any
            // parameter CV. Mutation or reference binding therefore defeats
            // every per-parameter uniqueness proof in this frame.
            OpCode::AssignDynamicVar
            | OpCode::UnsetDynamicVar
            | OpCode::BindDynamicVarRef
            | OpCode::AssignDynamicVarRef
            | OpCode::BindDynamicGlobal => return 0,
            // A direct return transfers a Value out of the frame. Aliases made
            // through another CV are owned clones and remain eligible.
            OpCode::Return if instruction.op1_type == OpType::Cv => {
                clear_cv(&mut mask, instruction.op1)
            }
            _ => {}
        }
    }
    mask
}

const SCALAR_LONG_PLAN_MAX_ARGS: u32 = 8;
const SCALAR_LONG_PLAN_MAX_OPS: usize = 8;

fn scalar_long_source(
    op_array: &OpArray,
    temporary_results: &HashMap<u16, ScalarLongSource>,
    this_offset: u32,
    public_args: u32,
    op_type: OpType,
    operand: u16,
) -> Option<ScalarLongSource> {
    match op_type {
        OpType::Cv => temporary_results.get(&operand).copied().or_else(|| {
            if operand as u32 >= this_offset && (operand as u32) < this_offset + public_args {
                Some(ScalarLongSource::Input(
                    (operand as u32 - this_offset) as u16,
                ))
            } else {
                None
            }
        }),
        OpType::Const => op_array
            .literals
            .get(operand as usize)
            .filter(|value| value.value_type() == crate::value::ValueType::Long)
            .and_then(Value::as_long)
            .map(ScalarLongSource::Constant),
        OpType::Tmp | OpType::Var => temporary_results.get(&operand).copied(),
        OpType::Unused => None,
    }
}

/// Recognize a small straight-line integer expression such as
/// `return ($a + 1) * $b`. This is deliberately narrower than general PHP
/// arithmetic: runtime Long guards and checked operations must all succeed or
/// the untouched canonical frame executes normally.
fn build_scalar_long_function_plan(function: &UserFunction) -> Option<Box<ScalarLongFunctionPlan>> {
    let common = &function.common;
    let op_array = &function.op_array;
    let public_args = common.sig.public_arity();
    if !common.supports_scalar_long_plan()
        || common.plan.ret != ReturnStrategy::Fast
        || public_args > SCALAR_LONG_PLAN_MAX_ARGS
        || op_array.instructions.len() > SCALAR_LONG_PLAN_MAX_OPS + 6
    {
        return None;
    }

    build_straight_scalar_long_function_plan(function)
        .or_else(|| build_conditional_scalar_long_function_plan(function))
}

#[derive(Clone, Copy)]
struct ProvenScalarDoubleSource {
    source: ScalarDoubleSource,
    is_double: bool,
}

fn scalar_double_source(
    op_array: &OpArray,
    temporary_results: &HashMap<u16, ProvenScalarDoubleSource>,
    this_offset: u32,
    public_args: u32,
    op_type: OpType,
    operand: u16,
) -> Option<ProvenScalarDoubleSource> {
    match op_type {
        OpType::Cv => {
            if operand as u32 >= this_offset && (operand as u32) < this_offset + public_args {
                Some(ProvenScalarDoubleSource {
                    source: ScalarDoubleSource::Input((operand as u32 - this_offset) as u16),
                    // The frame-free adapter admits this plan only after an
                    // exact raw-Double tag guard succeeds.
                    is_double: true,
                })
            } else {
                temporary_results.get(&operand).copied()
            }
        }
        OpType::Const => {
            let value = op_array.literals.get(operand as usize)?;
            match value.value_type() {
                crate::value::ValueType::Double => Some(ProvenScalarDoubleSource {
                    source: ScalarDoubleSource::Constant(value.as_double()?),
                    is_double: true,
                }),
                crate::value::ValueType::Long => Some(ProvenScalarDoubleSource {
                    source: ScalarDoubleSource::Constant(value.as_long()? as f64),
                    is_double: false,
                }),
                _ => None,
            }
        }
        OpType::Tmp | OpType::Var => temporary_results.get(&operand).copied(),
        OpType::Unused => None,
    }
}

fn scalar_double_op_kind(opcode: OpCode) -> Option<ScalarDoubleOpKind> {
    match opcode {
        OpCode::Add | OpCode::Add_TmpTmp | OpCode::Add_CvTmp => Some(ScalarDoubleOpKind::Add),
        OpCode::Sub | OpCode::Sub_CvConst | OpCode::Sub_TmpTmp => {
            Some(ScalarDoubleOpKind::Subtract)
        }
        OpCode::Mul => Some(ScalarDoubleOpKind::Multiply),
        OpCode::Div => Some(ScalarDoubleOpKind::Divide),
        _ => None,
    }
}

/// Recognize a straight-line expression whose runtime arguments are all exact
/// Double values. Every operation must already have a Double operand, which
/// excludes constant-only Long subexpressions whose overflow/type behavior
/// would differ from IEEE-754 arithmetic.
fn build_straight_scalar_double_function_plan(
    function: &UserFunction,
) -> Option<Box<ScalarDoubleFunctionPlan>> {
    let common = &function.common;
    let op_array = &function.op_array;
    let public_args = common.sig.public_arity();
    if !common.supports_scalar_double_plan()
        || common.plan.ret != ReturnStrategy::Fast
        || public_args > SCALAR_LONG_PLAN_MAX_ARGS
        || op_array.instructions.len() > SCALAR_LONG_PLAN_MAX_OPS + 6
    {
        return None;
    }

    let mut temporary_results = HashMap::new();
    let mut operations = Vec::new();
    for instruction in &op_array.instructions {
        if instruction.opcode == OpCode::ReleaseTemps {
            continue;
        }
        if instruction.opcode == OpCode::Return {
            if instruction.extended_value == 0 {
                return None;
            }
            let output = scalar_double_source(
                op_array,
                &temporary_results,
                common.sig.this_offset,
                public_args,
                instruction.op1_type,
                instruction.op1,
            )?;
            if !output.is_double {
                return None;
            }
            return Some(Box::new(ScalarDoubleFunctionPlan::new(
                public_args as u8,
                ScalarDoubleProgram {
                    operations: operations.into_boxed_slice(),
                    output: output.source,
                },
            )));
        }

        if instruction.opcode == OpCode::AssignCv {
            if instruction.op1_type != OpType::Cv {
                return None;
            }
            let destination = instruction.op1 as u32;
            let first_argument = common.sig.this_offset;
            let argument_end = first_argument + public_args;
            if destination < first_argument
                || (destination >= first_argument && destination < argument_end)
            {
                return None;
            }
            let source = scalar_double_source(
                op_array,
                &temporary_results,
                common.sig.this_offset,
                public_args,
                instruction.op2_type,
                instruction.op2,
            )?;
            temporary_results.insert(instruction.op1, source);
            continue;
        }

        let kind = scalar_double_op_kind(instruction.opcode)?;
        if operations.len() == SCALAR_LONG_PLAN_MAX_OPS
            || !matches!(instruction.result_type, OpType::Tmp | OpType::Var)
        {
            return None;
        }
        let lhs = scalar_double_source(
            op_array,
            &temporary_results,
            common.sig.this_offset,
            public_args,
            instruction.op1_type,
            instruction.op1,
        )?;
        let rhs = scalar_double_source(
            op_array,
            &temporary_results,
            common.sig.this_offset,
            public_args,
            instruction.op2_type,
            instruction.op2,
        )?;
        if !lhs.is_double && !rhs.is_double {
            return None;
        }
        let result_index = operations.len() as u8;
        operations.push(ScalarDoubleOp {
            kind,
            lhs: lhs.source,
            rhs: rhs.source,
        });
        temporary_results.insert(
            instruction.result,
            ProvenScalarDoubleSource {
                source: ScalarDoubleSource::Temporary(result_index),
                is_double: true,
            },
        );
    }
    None
}

fn append_scalar_double_operation(
    function: &UserFunction,
    instruction: &Instruction,
    temporary_results: &mut HashMap<u16, ProvenScalarDoubleSource>,
    operations: &mut Vec<ScalarDoubleOp>,
) -> Option<()> {
    let kind = scalar_double_op_kind(instruction.opcode)?;
    if operations.len() == SCALAR_LONG_PLAN_MAX_OPS
        || !matches!(instruction.result_type, OpType::Tmp | OpType::Var)
    {
        return None;
    }
    let public_args = function.common.sig.public_arity();
    let lhs = scalar_double_source(
        &function.op_array,
        temporary_results,
        function.common.sig.this_offset,
        public_args,
        instruction.op1_type,
        instruction.op1,
    )?;
    let rhs = scalar_double_source(
        &function.op_array,
        temporary_results,
        function.common.sig.this_offset,
        public_args,
        instruction.op2_type,
        instruction.op2,
    )?;
    if !lhs.is_double && !rhs.is_double {
        return None;
    }
    let result_index = operations.len() as u8;
    operations.push(ScalarDoubleOp {
        kind,
        lhs: lhs.source,
        rhs: rhs.source,
    });
    temporary_results.insert(
        instruction.result,
        ProvenScalarDoubleSource {
            source: ScalarDoubleSource::Temporary(result_index),
            is_double: true,
        },
    );
    Some(())
}

fn bind_scalar_double_local(
    function: &UserFunction,
    instruction: &Instruction,
    temporary_results: &mut HashMap<u16, ProvenScalarDoubleSource>,
) -> Option<()> {
    if instruction.opcode != OpCode::AssignCv || instruction.op1_type != OpType::Cv {
        return None;
    }
    let public_args = function.common.sig.public_arity();
    let destination = instruction.op1 as u32;
    let first_argument = function.common.sig.this_offset;
    let argument_end = first_argument + public_args;
    if destination < first_argument || (destination >= first_argument && destination < argument_end)
    {
        return None;
    }
    let source = scalar_double_source(
        &function.op_array,
        temporary_results,
        first_argument,
        public_args,
        instruction.op2_type,
        instruction.op2,
    )?;
    temporary_results.insert(instruction.op1, source);
    Some(())
}

fn scalar_double_return_arm(
    function: &UserFunction,
    start: usize,
    limit: usize,
    temporary_results: &mut HashMap<u16, ProvenScalarDoubleSource>,
    operations: &mut Vec<ScalarDoubleOp>,
) -> Option<ScalarDoubleSource> {
    for instruction in function.op_array.instructions.get(start..limit)? {
        if instruction.opcode == OpCode::ReleaseTemps {
            continue;
        }
        if instruction.opcode == OpCode::Return {
            if instruction.extended_value == 0 {
                return None;
            }
            let output = scalar_double_source(
                &function.op_array,
                temporary_results,
                function.common.sig.this_offset,
                function.common.sig.public_arity(),
                instruction.op1_type,
                instruction.op1,
            )?;
            return output.is_double.then_some(output.source);
        }
        if instruction.opcode == OpCode::AssignCv {
            bind_scalar_double_local(function, instruction, temporary_results)?;
            continue;
        }
        append_scalar_double_operation(function, instruction, temporary_results, operations)?;
    }
    None
}

/// Recognize one pure exact-Double guard clause or `if/else` whose two edges
/// return scalar arithmetic expressions. The two arm ranges stay disjoint so
/// the Rust evaluator and both native backends execute only the selected arm.
fn build_conditional_scalar_double_function_plan(
    function: &UserFunction,
) -> Option<Box<ScalarDoubleFunctionPlan>> {
    let instructions = &function.op_array.instructions;
    let public_args = function.common.sig.public_arity();
    if !function.common.supports_scalar_double_plan()
        || function.common.plan.ret != ReturnStrategy::Fast
        || public_args > SCALAR_LONG_PLAN_MAX_ARGS
        || instructions.len() > 32
    {
        return None;
    }

    let mut temporary_results = HashMap::new();
    let mut operations = Vec::new();
    let mut ip = 0usize;
    while let Some(instruction) = instructions.get(ip) {
        if instruction.opcode == OpCode::ReleaseTemps {
            ip += 1;
            continue;
        }
        if scalar_double_op_kind(instruction.opcode).is_some() {
            append_scalar_double_operation(
                function,
                instruction,
                &mut temporary_results,
                &mut operations,
            )?;
            ip += 1;
            continue;
        }
        if instruction.opcode == OpCode::AssignCv {
            bind_scalar_double_local(function, instruction, &mut temporary_results)?;
            ip += 1;
            continue;
        }
        break;
    }

    let condition_instruction = *instructions.get(ip)?;
    let condition_sources = |instruction: Instruction| {
        let lhs = scalar_double_source(
            &function.op_array,
            &temporary_results,
            function.common.sig.this_offset,
            public_args,
            instruction.op1_type,
            instruction.op1,
        )?;
        let rhs = scalar_double_source(
            &function.op_array,
            &temporary_results,
            function.common.sig.this_offset,
            public_args,
            instruction.op2_type,
            instruction.op2,
        )?;
        (lhs.is_double || rhs.is_double).then_some((lhs.source, rhs.source))
    };
    let (kind, lhs, rhs, branch_ip, fused_jump_target) = match condition_instruction.opcode {
        OpCode::IsEqual => {
            let (lhs, rhs) = condition_sources(condition_instruction)?;
            (ScalarLongConditionKind::Equal, lhs, rhs, ip + 1, None)
        }
        OpCode::IsNotEqual => {
            let (lhs, rhs) = condition_sources(condition_instruction)?;
            (ScalarLongConditionKind::NotEqual, lhs, rhs, ip + 1, None)
        }
        OpCode::IsSmaller | OpCode::IsSmaller_CvConst => {
            let (lhs, rhs) = condition_sources(condition_instruction)?;
            (ScalarLongConditionKind::LessThan, lhs, rhs, ip + 1, None)
        }
        OpCode::IsSmallerOrEqual | OpCode::IsSmallerOrEqual_CvConst => {
            let (lhs, rhs) = condition_sources(condition_instruction)?;
            (
                ScalarLongConditionKind::LessThanOrEqual,
                lhs,
                rhs,
                ip + 1,
                None,
            )
        }
        OpCode::JmpZ => {
            let lhs = scalar_double_source(
                &function.op_array,
                &temporary_results,
                function.common.sig.this_offset,
                public_args,
                condition_instruction.op1_type,
                condition_instruction.op1,
            )?;
            if !lhs.is_double {
                return None;
            }
            (
                ScalarLongConditionKind::NotEqual,
                lhs.source,
                ScalarDoubleSource::Constant(0.0),
                ip,
                None,
            )
        }
        OpCode::JmpZ_Eq_CvConst | OpCode::JmpZ_Lt_CvConst | OpCode::JmpZ_Le_CvConst => {
            let (lhs, rhs) = condition_sources(condition_instruction)?;
            (
                match condition_instruction.opcode {
                    OpCode::JmpZ_Eq_CvConst => ScalarLongConditionKind::Equal,
                    OpCode::JmpZ_Lt_CvConst => ScalarLongConditionKind::LessThan,
                    OpCode::JmpZ_Le_CvConst => ScalarLongConditionKind::LessThanOrEqual,
                    _ => unreachable!(),
                },
                lhs,
                rhs,
                ip,
                Some(condition_instruction.result as usize),
            )
        }
        _ => return None,
    };

    let (when_true_ip, when_false_ip) = if let Some(target) = fused_jump_target {
        (ip + 2, target)
    } else {
        let branch = instructions.get(branch_ip)?;
        if branch.opcode != OpCode::JmpZ {
            return None;
        }
        if branch_ip != ip
            && (!matches!(condition_instruction.result_type, OpType::Tmp | OpType::Var)
                || branch.op1_type != condition_instruction.result_type
                || branch.op1 != condition_instruction.result)
        {
            return None;
        }
        (branch_ip + 1, branch.op2 as usize)
    };
    if when_true_ip >= when_false_ip || when_false_ip >= instructions.len() {
        return None;
    }

    let shared_operation_count = operations.len();
    let branch_results = temporary_results;
    let mut when_true_results = branch_results.clone();
    let when_true = scalar_double_return_arm(
        function,
        when_true_ip,
        when_false_ip,
        &mut when_true_results,
        &mut operations,
    )?;
    let when_true_operation_count = operations.len() - shared_operation_count;
    let mut when_false_results = branch_results;
    let when_false = scalar_double_return_arm(
        function,
        when_false_ip,
        instructions.len(),
        &mut when_false_results,
        &mut operations,
    )?;
    let when_false_operation_count =
        operations.len() - shared_operation_count - when_true_operation_count;

    Some(Box::new(ScalarDoubleFunctionPlan::new_conditional(
        public_args as u8,
        ScalarDoubleProgram {
            operations: operations.into_boxed_slice(),
            output: when_true,
        },
        ScalarDoubleSelect {
            kind,
            lhs,
            rhs,
            shared_operation_count: shared_operation_count as u8,
            when_true_operation_count: when_true_operation_count as u8,
            when_false_operation_count: when_false_operation_count as u8,
            when_true,
            when_false,
            merge_result: false,
        },
    )))
}

fn build_scalar_double_function_plan(
    function: &UserFunction,
) -> Option<Box<ScalarDoubleFunctionPlan>> {
    build_straight_scalar_double_function_plan(function)
        .or_else(|| build_conditional_scalar_double_function_plan(function))
}

const COMPOSED_SCALAR_DOUBLE_PLAN_MAX_OPS: usize = 16;

/// Recognize a straight-line exact-Double body containing direct function
/// calls or same-receiver `$this` method calls. Calls remain guarded IR nodes
/// here; the runtime resolves their canonical inline caches and flattens
/// proven Double callees before execution.
fn build_composed_scalar_double_function_plan(
    function: &UserFunction,
) -> Option<Box<ComposedScalarDoubleFunctionPlan>> {
    let common = &function.common;
    let op_array = &function.op_array;
    let public_args = common.sig.public_arity();
    if !common.supports_scalar_double_plan()
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
        if instruction.opcode == OpCode::ReleaseTemps {
            ip += 1;
            continue;
        }
        if instruction.opcode == OpCode::Return {
            if instruction.extended_value == 0 || !contains_call {
                return None;
            }
            let output = scalar_double_source(
                op_array,
                &temporary_results,
                common.sig.this_offset,
                public_args,
                instruction.op1_type,
                instruction.op1,
            )?;
            if !output.is_double {
                return None;
            }
            return Some(Box::new(ComposedScalarDoubleFunctionPlan {
                public_args: public_args as u8,
                operations: operations.into_boxed_slice(),
                output: output.source,
            }));
        }

        if matches!(
            instruction.opcode,
            OpCode::InitFcall | OpCode::InitMethodCall
        ) {
            let (argument_count, parameter_offset, guard) = match instruction.opcode {
                OpCode::InitFcall => (
                    instruction.op1 as usize,
                    0usize,
                    ScalarLongCallGuard::FunctionCache {
                        cache_ip: ip as u32,
                    },
                ),
                OpCode::InitMethodCall
                    if common.sig.this_offset == 1
                        && instruction.op1_type == OpType::Cv
                        && instruction.op1 == 0 =>
                {
                    (
                        instruction.extended_value as usize,
                        1usize,
                        ScalarLongCallGuard::MethodCache {
                            cache_ip: ip as u32,
                            receiver_slot: 0,
                        },
                    )
                }
                _ => return None,
            };
            if argument_count > SCALAR_LONG_PLAN_MAX_ARGS as usize
                || ip > u32::MAX as usize
                || operations.len() == COMPOSED_SCALAR_DOUBLE_PLAN_MAX_OPS
                || ip + argument_count + 1 >= op_array.instructions.len()
            {
                return None;
            }
            let mut arguments = Vec::with_capacity(argument_count);
            for argument_index in 0..argument_count {
                let send = &op_array.instructions[ip + 1 + argument_index];
                if !matches!(send.opcode, OpCode::SendVal | OpCode::SendVarEx)
                    || send.op2 as usize != argument_index + parameter_offset
                {
                    return None;
                }
                let source = scalar_double_source(
                    op_array,
                    &temporary_results,
                    common.sig.this_offset,
                    public_args,
                    send.op1_type,
                    send.op1,
                )?;
                // A Long literal would require the canonical weak-float
                // coercion boundary before entering the nested exact ABI.
                if !source.is_double {
                    return None;
                }
                arguments.push(source.source);
            }
            let do_fcall = &op_array.instructions[ip + 1 + argument_count];
            if do_fcall.opcode != OpCode::DoFcall
                || !matches!(do_fcall.result_type, OpType::Tmp | OpType::Var)
            {
                return None;
            }
            let result_index = operations.len() as u8;
            operations.push(ComposedScalarDoubleOp::Call(ScalarDoubleCall {
                guard,
                arguments: arguments.into_boxed_slice(),
            }));
            temporary_results.insert(
                do_fcall.result,
                ProvenScalarDoubleSource {
                    source: ScalarDoubleSource::Temporary(result_index),
                    is_double: true,
                },
            );
            contains_call = true;
            ip += argument_count + 2;
            continue;
        }

        if instruction.opcode == OpCode::AssignCv {
            if instruction.op1_type != OpType::Cv {
                return None;
            }
            let destination = instruction.op1 as u32;
            let first_argument = common.sig.this_offset;
            let argument_end = first_argument + public_args;
            if destination < first_argument
                || (destination >= first_argument && destination < argument_end)
            {
                return None;
            }
            let source = scalar_double_source(
                op_array,
                &temporary_results,
                common.sig.this_offset,
                public_args,
                instruction.op2_type,
                instruction.op2,
            )?;
            temporary_results.insert(instruction.op1, source);
            ip += 1;
            continue;
        }

        let kind = scalar_double_op_kind(instruction.opcode)?;
        if operations.len() == COMPOSED_SCALAR_DOUBLE_PLAN_MAX_OPS
            || !matches!(instruction.result_type, OpType::Tmp | OpType::Var)
        {
            return None;
        }
        let lhs = scalar_double_source(
            op_array,
            &temporary_results,
            common.sig.this_offset,
            public_args,
            instruction.op1_type,
            instruction.op1,
        )?;
        let rhs = scalar_double_source(
            op_array,
            &temporary_results,
            common.sig.this_offset,
            public_args,
            instruction.op2_type,
            instruction.op2,
        )?;
        if !lhs.is_double && !rhs.is_double {
            return None;
        }
        let result_index = operations.len() as u8;
        operations.push(ComposedScalarDoubleOp::Arithmetic(ScalarDoubleOp {
            kind,
            lhs: lhs.source,
            rhs: rhs.source,
        }));
        temporary_results.insert(
            instruction.result,
            ProvenScalarDoubleSource {
                source: ScalarDoubleSource::Temporary(result_index),
                is_double: true,
            },
        );
        ip += 1;
    }
    None
}

fn scalar_long_op_kind(opcode: OpCode) -> Option<ScalarLongOpKind> {
    match opcode {
        OpCode::Add | OpCode::Add_TmpTmp | OpCode::Add_CvTmp => Some(ScalarLongOpKind::Add),
        OpCode::Sub | OpCode::Sub_CvConst | OpCode::Sub_TmpTmp => Some(ScalarLongOpKind::Subtract),
        OpCode::Mul => Some(ScalarLongOpKind::Multiply),
        OpCode::Mod | OpCode::Mod_LongLong => Some(ScalarLongOpKind::Modulo),
        OpCode::BitwiseAnd | OpCode::BitwiseAnd_LongLong => Some(ScalarLongOpKind::BitwiseAnd),
        OpCode::BitwiseOr | OpCode::BitwiseOr_LongLong => Some(ScalarLongOpKind::BitwiseOr),
        OpCode::BitwiseXor | OpCode::BitwiseXor_LongLong => Some(ScalarLongOpKind::BitwiseXor),
        _ => None,
    }
}

fn scalar_long_instruction_kind(instruction: &Instruction) -> Option<ScalarLongOpKind> {
    if instruction.opcode == OpCode::Spaceship {
        Some(ScalarLongOpKind::Compare)
    } else if instruction.opcode == OpCode::DirectInternalCall2
        && crate::builtin_metadata::DirectInternalKind::from_id(instruction.extended_value)
            == Some(crate::builtin_metadata::DirectInternalKind::Intdiv)
    {
        Some(ScalarLongOpKind::IntDivide)
    } else {
        scalar_long_op_kind(instruction.opcode)
    }
}

fn append_scalar_long_operation(
    function: &UserFunction,
    instruction: &Instruction,
    temporary_results: &mut HashMap<u16, ScalarLongSource>,
    operations: &mut Vec<ScalarLongOp>,
) -> Option<()> {
    let kind = scalar_long_instruction_kind(instruction)?;
    if operations.len() == SCALAR_LONG_PLAN_MAX_OPS
        || !matches!(instruction.result_type, OpType::Tmp | OpType::Var)
    {
        return None;
    }
    let common = &function.common;
    let op_array = &function.op_array;
    let public_args = common.sig.public_arity();
    let lhs = scalar_long_source(
        op_array,
        temporary_results,
        common.sig.this_offset,
        public_args,
        instruction.op1_type,
        instruction.op1,
    )?;
    let rhs = scalar_long_source(
        op_array,
        temporary_results,
        common.sig.this_offset,
        public_args,
        instruction.op2_type,
        instruction.op2,
    )?;
    let result_index = operations.len() as u8;
    operations.push(ScalarLongOp { kind, lhs, rhs });
    temporary_results.insert(
        instruction.result,
        ScalarLongSource::Temporary(result_index),
    );
    Some(())
}

fn bind_scalar_long_local(
    function: &UserFunction,
    instruction: &Instruction,
    temporary_results: &mut HashMap<u16, ScalarLongSource>,
) -> Option<()> {
    if instruction.opcode != OpCode::AssignCv || instruction.op1_type != OpType::Cv {
        return None;
    }
    let destination = instruction.op1 as u32;
    let first_argument = function.common.sig.this_offset;
    let argument_end = first_argument + function.common.sig.public_arity();
    // Mutating `$this` or a public parameter changes what later external Input
    // sources mean. Keep those CVs canonical; local CVs are aliases here.
    if destination < first_argument || (destination >= first_argument && destination < argument_end)
    {
        return None;
    }
    let source = scalar_long_source(
        &function.op_array,
        temporary_results,
        function.common.sig.this_offset,
        function.common.sig.public_arity(),
        instruction.op2_type,
        instruction.op2,
    )?;
    temporary_results.insert(instruction.op1, source);
    Some(())
}

fn build_straight_scalar_long_function_plan(
    function: &UserFunction,
) -> Option<Box<ScalarLongFunctionPlan>> {
    let common = &function.common;
    let op_array = &function.op_array;
    let public_args = common.sig.public_arity();

    let mut temporary_results = HashMap::new();
    let mut operations = Vec::new();

    for instruction in &op_array.instructions {
        if instruction.opcode == OpCode::ReleaseTemps {
            // The scalar plan never materializes canonical Value slots; the
            // cleanup remains relevant only on baseline fallback.
            continue;
        }
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
            return Some(Box::new(ScalarLongFunctionPlan::new(
                public_args as u8,
                ScalarLongProgram {
                    operations: operations.into_boxed_slice(),
                    outputs: [result],
                    output_count: 1,
                },
                None,
            )));
        }

        if instruction.opcode == OpCode::AssignCv {
            bind_scalar_long_local(function, instruction, &mut temporary_results)?;
            continue;
        }
        append_scalar_long_operation(
            function,
            instruction,
            &mut temporary_results,
            &mut operations,
        )?;
    }

    None
}

/// Recognize a straight-line, side-effect-free Long callback whose only
/// observable effect is one final assignment to its first by-reference
/// parameter. Array consumers may execute this proof directly while every
/// type, overflow, or opcode mismatch resumes the canonical callback at the
/// first untouched member.
pub(crate) fn build_scalar_long_reference_mutation_plan(
    function: &UserFunction,
    capture_count: usize,
) -> Option<Box<ScalarLongFunctionPlan>> {
    let common = &function.common;
    let op_array = &function.op_array;
    let public_args = common.sig.public_arity();
    let total_inputs = (public_args as usize).checked_add(capture_count)?;
    if common.plan.call != CallStrategy::Fast
        || common.plan.ret != ReturnStrategy::Fast
        || !matches!(common.sig.return_type_hint, ParamTypeHint::None)
        || public_args != 2
        || !common.sig.is_param_by_ref(0)
        || common.sig.is_param_by_ref(1)
        || common.sig.param_type_hints.iter().any(|hint| {
            !matches!(
                hint,
                ParamTypeHint::None | ParamTypeHint::Mixed | ParamTypeHint::Int
            )
        })
        || total_inputs > SCALAR_LONG_PLAN_MAX_ARGS as usize
        || op_array.instructions.len() > SCALAR_LONG_PLAN_MAX_OPS + 6
    {
        return None;
    }

    let first_argument = common.sig.this_offset;
    let argument_end = first_argument + public_args;
    let mut temporary_results = HashMap::new();
    let capture_start = common.sig.parameter_cv_count();
    let capture_end = capture_start.checked_add(u32::try_from(capture_count).ok()?)?;
    if capture_end > op_array.num_cvs {
        return None;
    }
    for capture in 0..capture_count {
        temporary_results.insert(
            u16::try_from(capture_start as usize + capture).ok()?,
            ScalarLongSource::Input(u16::try_from(public_args as usize + capture).ok()?),
        );
    }
    let mut operations = Vec::new();
    let mut output = None;

    for instruction in &op_array.instructions {
        if instruction.opcode == OpCode::ReleaseTemps {
            continue;
        }
        if instruction.opcode == OpCode::Return {
            if instruction.extended_value != 0 {
                return None;
            }
            let output = output?;
            return Some(Box::new(ScalarLongFunctionPlan::new(
                u8::try_from(total_inputs).ok()?,
                ScalarLongProgram {
                    operations: operations.into_boxed_slice(),
                    outputs: [output],
                    output_count: 1,
                },
                None,
            )));
        }
        // The mutation must be final. This makes every preceding input source
        // the original argument value and leaves no post-write behavior to
        // reproduce outside the canonical frame.
        if output.is_some() {
            return None;
        }
        if instruction.opcode == OpCode::AssignCv {
            if instruction.op1_type != OpType::Cv {
                return None;
            }
            let destination = instruction.op1 as u32;
            if destination == first_argument {
                output = Some(scalar_long_source(
                    op_array,
                    &temporary_results,
                    first_argument,
                    public_args,
                    instruction.op2_type,
                    instruction.op2,
                )?);
            } else {
                if destination < first_argument
                    || destination < argument_end
                    || (destination >= capture_start && destination < capture_end)
                {
                    return None;
                }
                let source = scalar_long_source(
                    op_array,
                    &temporary_results,
                    first_argument,
                    public_args,
                    instruction.op2_type,
                    instruction.op2,
                )?;
                temporary_results.insert(instruction.op1, source);
            }
            continue;
        }
        append_scalar_long_operation(
            function,
            instruction,
            &mut temporary_results,
            &mut operations,
        )?;
    }
    None
}

fn scalar_long_condition_operand(
    function: &UserFunction,
    temporary_results: &HashMap<u16, ScalarLongSource>,
    masked_results: &HashMap<u16, ScalarLongConditionOperand>,
    op_type: OpType,
    operand: u16,
) -> Option<ScalarLongConditionOperand> {
    if matches!(op_type, OpType::Tmp | OpType::Var) {
        if let Some(masked) = masked_results.get(&operand) {
            return Some(*masked);
        }
    }
    scalar_long_source(
        &function.op_array,
        temporary_results,
        function.common.sig.this_offset,
        function.common.sig.public_arity(),
        op_type,
        operand,
    )
    .map(ScalarLongConditionOperand::Source)
}

fn scalar_long_return_arm(
    function: &UserFunction,
    start: usize,
    limit: usize,
    temporary_results: &mut HashMap<u16, ScalarLongSource>,
    operations: &mut Vec<ScalarLongOp>,
) -> Option<ScalarLongSource> {
    for instruction in function.op_array.instructions.get(start..limit)? {
        if instruction.opcode == OpCode::ReleaseTemps {
            continue;
        }
        if instruction.opcode == OpCode::Return {
            if instruction.extended_value == 0 {
                return None;
            }
            return scalar_long_source(
                &function.op_array,
                temporary_results,
                function.common.sig.this_offset,
                function.common.sig.public_arity(),
                instruction.op1_type,
                instruction.op1,
            );
        }
        if instruction.opcode == OpCode::AssignCv {
            bind_scalar_long_local(function, instruction, temporary_results)?;
            continue;
        }
        append_scalar_long_operation(function, instruction, temporary_results, operations)?;
    }
    None
}

/// Recognize a pure Long guard clause or `if` body whose two control-flow
/// edges return scalar expressions. Runtime still validates raw Long inputs
/// and checked arithmetic; this representation only removes branch bytecode
/// dispatch and the callee ExecuteData frame.
fn build_conditional_scalar_long_function_plan(
    function: &UserFunction,
) -> Option<Box<ScalarLongFunctionPlan>> {
    let instructions = &function.op_array.instructions;
    let public_args = function.common.sig.public_arity();
    let mut temporary_results = HashMap::new();
    let mut masked_results = HashMap::new();
    let mut operations = Vec::new();
    let mut ip = 0usize;

    // Allow shared scalar arithmetic and bit masks before the comparison. A
    // mask remains a predicate operand because PHP integer `&` cannot fail or
    // overflow once both Long guards have succeeded.
    while let Some(instruction) = instructions.get(ip) {
        if instruction.opcode == OpCode::ReleaseTemps {
            ip += 1;
            continue;
        }
        if instruction.opcode == OpCode::BitwiseAnd {
            if !matches!(instruction.result_type, OpType::Tmp | OpType::Var) {
                return None;
            }
            let lhs = scalar_long_condition_operand(
                function,
                &temporary_results,
                &masked_results,
                instruction.op1_type,
                instruction.op1,
            )?;
            let rhs = scalar_long_condition_operand(
                function,
                &temporary_results,
                &masked_results,
                instruction.op2_type,
                instruction.op2,
            )?;
            let ScalarLongConditionOperand::Source(lhs) = lhs else {
                return None;
            };
            let ScalarLongConditionOperand::Source(rhs) = rhs else {
                return None;
            };
            masked_results.insert(
                instruction.result,
                ScalarLongConditionOperand::BitwiseAnd { lhs, rhs },
            );
            ip += 1;
            continue;
        }
        if scalar_long_instruction_kind(instruction).is_some() {
            append_scalar_long_operation(
                function,
                instruction,
                &mut temporary_results,
                &mut operations,
            )?;
            ip += 1;
            continue;
        }
        if instruction.opcode == OpCode::AssignCv {
            bind_scalar_long_local(function, instruction, &mut temporary_results)?;
            ip += 1;
            continue;
        }
        break;
    }

    let condition_instruction = *instructions.get(ip)?;
    let (kind, lhs, rhs, branch_ip, fused_jump_target) = match condition_instruction.opcode {
        OpCode::IsEqual | OpCode::IsIdentical | OpCode::IsEqual_CvConst => (
            ScalarLongConditionKind::Equal,
            scalar_long_condition_operand(
                function,
                &temporary_results,
                &masked_results,
                condition_instruction.op1_type,
                condition_instruction.op1,
            )?,
            scalar_long_condition_operand(
                function,
                &temporary_results,
                &masked_results,
                condition_instruction.op2_type,
                condition_instruction.op2,
            )?,
            ip + 1,
            None,
        ),
        OpCode::IsNotEqual | OpCode::IsNotIdentical => (
            ScalarLongConditionKind::NotEqual,
            scalar_long_condition_operand(
                function,
                &temporary_results,
                &masked_results,
                condition_instruction.op1_type,
                condition_instruction.op1,
            )?,
            scalar_long_condition_operand(
                function,
                &temporary_results,
                &masked_results,
                condition_instruction.op2_type,
                condition_instruction.op2,
            )?,
            ip + 1,
            None,
        ),
        OpCode::IsSmaller | OpCode::IsSmaller_CvConst => (
            ScalarLongConditionKind::LessThan,
            scalar_long_condition_operand(
                function,
                &temporary_results,
                &masked_results,
                condition_instruction.op1_type,
                condition_instruction.op1,
            )?,
            scalar_long_condition_operand(
                function,
                &temporary_results,
                &masked_results,
                condition_instruction.op2_type,
                condition_instruction.op2,
            )?,
            ip + 1,
            None,
        ),
        OpCode::IsSmallerOrEqual | OpCode::IsSmallerOrEqual_CvConst => (
            ScalarLongConditionKind::LessThanOrEqual,
            scalar_long_condition_operand(
                function,
                &temporary_results,
                &masked_results,
                condition_instruction.op1_type,
                condition_instruction.op1,
            )?,
            scalar_long_condition_operand(
                function,
                &temporary_results,
                &masked_results,
                condition_instruction.op2_type,
                condition_instruction.op2,
            )?,
            ip + 1,
            None,
        ),
        OpCode::JmpZ => (
            ScalarLongConditionKind::NotEqual,
            scalar_long_condition_operand(
                function,
                &temporary_results,
                &masked_results,
                condition_instruction.op1_type,
                condition_instruction.op1,
            )?,
            ScalarLongConditionOperand::Source(ScalarLongSource::Constant(0)),
            ip,
            None,
        ),
        OpCode::JmpZ_Eq_CvConst | OpCode::JmpZ_Lt_CvConst | OpCode::JmpZ_Le_CvConst => (
            match condition_instruction.opcode {
                OpCode::JmpZ_Eq_CvConst => ScalarLongConditionKind::Equal,
                OpCode::JmpZ_Lt_CvConst => ScalarLongConditionKind::LessThan,
                OpCode::JmpZ_Le_CvConst => ScalarLongConditionKind::LessThanOrEqual,
                _ => unreachable!(),
            },
            scalar_long_condition_operand(
                function,
                &temporary_results,
                &masked_results,
                condition_instruction.op1_type,
                condition_instruction.op1,
            )?,
            scalar_long_condition_operand(
                function,
                &temporary_results,
                &masked_results,
                condition_instruction.op2_type,
                condition_instruction.op2,
            )?,
            ip,
            Some(condition_instruction.result as usize),
        ),
        _ => return None,
    };
    let shared_operation_count = operations.len();

    let (when_true_ip, when_false_ip) = if let Some(target) = fused_jump_target {
        // Fused comparisons retain the original JmpZ in the canonical stream
        // and skip over it on fall-through.
        (ip + 2, target)
    } else {
        let branch = instructions.get(branch_ip)?;
        if branch.opcode != OpCode::JmpZ {
            return None;
        }
        if branch_ip != ip
            && (!matches!(condition_instruction.result_type, OpType::Tmp | OpType::Var)
                || branch.op1_type != condition_instruction.result_type
                || branch.op1 != condition_instruction.result)
        {
            return None;
        }
        (branch_ip + 1, branch.op2 as usize)
    };
    if when_true_ip >= when_false_ip || when_false_ip >= instructions.len() {
        return None;
    }

    let branch_results = temporary_results;
    let mut when_true_results = branch_results.clone();
    let true_returns = instructions[when_true_ip..when_false_ip]
        .iter()
        .any(|instruction| instruction.opcode == OpCode::Return);
    let (when_true, when_false, when_true_operation_count) = if true_returns {
        let when_true = scalar_long_return_arm(
            function,
            when_true_ip,
            when_false_ip,
            &mut when_true_results,
            &mut operations,
        )?;
        let when_true_operation_count = operations.len() - shared_operation_count;
        let mut when_false_results = branch_results;
        let when_false = scalar_long_return_arm(
            function,
            when_false_ip,
            instructions.len(),
            &mut when_false_results,
            &mut operations,
        )?;
        (when_true, when_false, when_true_operation_count)
    } else {
        // Canonical `if ($cond) { $value = expr; } return $value;` has a
        // shared return at the false/join target. Model it as a select between
        // the post-body binding and the incoming binding without inventing a
        // second control-flow representation.
        for instruction in &instructions[when_true_ip..when_false_ip] {
            if instruction.opcode == OpCode::ReleaseTemps {
                continue;
            } else if instruction.opcode == OpCode::AssignCv {
                bind_scalar_long_local(function, instruction, &mut when_true_results)?;
            } else {
                append_scalar_long_operation(
                    function,
                    instruction,
                    &mut when_true_results,
                    &mut operations,
                )?;
            }
        }
        let shared_return = instructions.get(when_false_ip)?;
        if shared_return.opcode != OpCode::Return || shared_return.extended_value == 0 {
            return None;
        }
        let when_true = scalar_long_source(
            &function.op_array,
            &when_true_results,
            function.common.sig.this_offset,
            public_args,
            shared_return.op1_type,
            shared_return.op1,
        )?;
        let when_false = scalar_long_source(
            &function.op_array,
            &branch_results,
            function.common.sig.this_offset,
            public_args,
            shared_return.op1_type,
            shared_return.op1,
        )?;
        (
            when_true,
            when_false,
            operations.len() - shared_operation_count,
        )
    };

    Some(Box::new(ScalarLongFunctionPlan::new(
        public_args as u8,
        ScalarLongProgram {
            operations: operations.into_boxed_slice(),
            outputs: [when_true],
            output_count: 1,
        },
        Some(ScalarLongSelect {
            kind,
            lhs,
            rhs,
            shared_operation_count: shared_operation_count as u8,
            when_true_operation_count: when_true_operation_count as u8,
            when_true,
            when_false,
        }),
    )))
}

const OBJECT_LONG_PLAN_MAX_ARGS: u32 = 8;
const OBJECT_LONG_PLAN_MAX_SLOTS: u32 = 64;
const OBJECT_LONG_PLAN_MAX_OPS: usize = 64;

fn object_long_source(
    function: &UserFunction,
    initialized: &[bool; OBJECT_LONG_PLAN_MAX_SLOTS as usize],
    long_argument_mask: &mut u8,
    op_type: OpType,
    operand: u16,
) -> Option<ObjectLongSource> {
    match op_type {
        OpType::Const => function
            .op_array
            .literals
            .get(operand as usize)
            .and_then(Value::as_long)
            .map(ObjectLongSource::Constant),
        OpType::Cv => {
            let slot = operand as u32;
            let first_argument = function.common.sig.this_offset;
            let argument_end = first_argument + function.common.sig.public_arity();
            if slot >= first_argument && slot < argument_end {
                *long_argument_mask |= 1 << (slot - first_argument);
                Some(ObjectLongSource::Slot(operand))
            } else if initialized.get(slot as usize).copied().unwrap_or(false) {
                Some(ObjectLongSource::Slot(operand))
            } else {
                None
            }
        }
        OpType::Tmp | OpType::Var => initialized
            .get(operand as usize)
            .copied()
            .filter(|initialized| *initialized)
            .map(|_| ObjectLongSource::Slot(operand)),
        OpType::Unused => None,
    }
}

fn object_long_intdiv_arm(
    operations: &[ObjectLongOp],
) -> Option<(ObjectLongSource, ObjectLongIntDivArm)> {
    let [
        ObjectLongOp::Arithmetic {
            kind: ScalarLongOpKind::Multiply,
            lhs,
            rhs,
            destination: multiplied,
        },
        ObjectLongOp::IntDiv {
            lhs: ObjectLongSource::Slot(dividend),
            rhs: ObjectLongSource::Constant(divisor),
            destination: divided,
        },
        ObjectLongOp::Return {
            value: ObjectLongSource::Slot(returned),
        },
    ] = operations
    else {
        return None;
    };
    if multiplied != dividend || divided != returned || *divisor == 0 {
        return None;
    }
    let (input, multiplier) = match (*lhs, *rhs) {
        (input @ ObjectLongSource::Slot(_), ObjectLongSource::Constant(multiplier))
        | (ObjectLongSource::Constant(multiplier), input @ ObjectLongSource::Slot(_)) => {
            (input, multiplier)
        }
        _ => return None,
    };
    Some((
        input,
        ObjectLongIntDivArm {
            multiplier,
            divisor: *divisor,
        },
    ))
}

fn build_object_long_string_intdiv_select(
    operations: &[ObjectLongOp],
) -> Option<Box<ObjectLongStringIntDivSelect>> {
    let mut ip = 0usize;
    let mut string_argument = None;
    let mut input = None;
    let mut cases = Vec::new();

    while let Some(ObjectLongOp::StringLiteralBranch {
        argument,
        literal,
        jump_when_equal: false,
        target,
    }) = operations.get(ip)
    {
        let target = *target as usize;
        if !matches!(operations.get(ip + 1), Some(ObjectLongOp::Noop))
            || target <= ip + 2
            || target > operations.len()
            || cases.len() == 8
        {
            return None;
        }
        let (arm_input, arm) = object_long_intdiv_arm(&operations[ip + 2..target])?;
        if string_argument
            .replace(*argument)
            .is_some_and(|found| found != *argument)
            || input
                .replace(arm_input)
                .is_some_and(|found| found != arm_input)
        {
            return None;
        }
        cases.push(ObjectLongStringIntDivCase {
            literal: *literal,
            arm,
        });
        ip = target;
    }
    if cases.is_empty() {
        return None;
    }

    let remaining = operations.get(ip..)?;
    let remaining = match remaining.last() {
        Some(ObjectLongOp::Bail) => &remaining[..remaining.len() - 1],
        _ => remaining,
    };
    let (default_input, default_arm) = object_long_intdiv_arm(remaining)?;
    if input != Some(default_input) {
        return None;
    }

    Some(Box::new(ObjectLongStringIntDivSelect {
        string_argument: string_argument?,
        input: default_input,
        cases: cases.into_boxed_slice(),
        default_arm,
    }))
}

/// Recognize the canonical short-circuit shape produced for
/// `($x % C) == K || ...` followed by two constant integer return arms.
/// The semantic ObjectLong program remains authoritative for every rejected
/// shape and for checked-remainder failure at runtime.
fn build_object_long_modulo_any_select(
    operations: &[ObjectLongOp],
) -> Option<Box<ObjectLongModuloAnySelect>> {
    let mut terms = Vec::new();
    let mut ip = 0usize;
    let mut match_target = None;

    while terms.len() < 8 && ip + 2 < operations.len() {
        let ObjectLongOp::Arithmetic {
            kind: ScalarLongOpKind::Modulo,
            lhs: input,
            rhs: ObjectLongSource::Constant(divisor),
            destination: remainder,
        } = operations[ip]
        else {
            break;
        };
        let ObjectLongOp::Compare {
            kind: ScalarLongConditionKind::Equal,
            lhs,
            rhs,
            destination: condition,
        } = operations[ip + 1]
        else {
            break;
        };
        let expected = match (lhs, rhs) {
            (ObjectLongSource::Slot(slot), ObjectLongSource::Constant(expected))
                if slot == remainder =>
            {
                expected
            }
            (ObjectLongSource::Constant(expected), ObjectLongSource::Slot(slot))
                if slot == remainder =>
            {
                expected
            }
            _ => break,
        };
        let ObjectLongOp::JumpIfTrue {
            condition: ObjectLongSource::Slot(jump_condition),
            target,
        } = operations[ip + 2]
        else {
            break;
        };
        if jump_condition != condition
            || match_target.is_some_and(|match_target| match_target != target)
        {
            break;
        }
        match_target = Some(target);
        terms.push(ObjectLongModuloEqualTerm {
            input,
            divisor,
            expected,
        });
        ip += 3;
    }

    if terms.is_empty() || ip + 5 >= operations.len() {
        return None;
    }
    let ObjectLongOp::Assign {
        destination: boolean_slot,
        source: ObjectLongSource::Constant(miss_flag),
    } = operations[ip]
    else {
        return None;
    };
    let ObjectLongOp::Jump {
        target: branch_target,
    } = operations[ip + 1]
    else {
        return None;
    };
    if match_target? as usize != ip + 2 || branch_target as usize != ip + 3 {
        return None;
    }
    let ObjectLongOp::Assign {
        destination: match_slot,
        source: ObjectLongSource::Constant(match_flag),
    } = operations[ip + 2]
    else {
        return None;
    };
    if match_slot != boolean_slot {
        return None;
    }

    let branch_ip = ip + 3;
    let (jump_when_true, return_target) = match operations[branch_ip] {
        ObjectLongOp::JumpIfFalse {
            condition: ObjectLongSource::Slot(slot),
            target,
        } if slot == boolean_slot => (false, target as usize),
        ObjectLongOp::JumpIfTrue {
            condition: ObjectLongSource::Slot(slot),
            target,
        } if slot == boolean_slot => (true, target as usize),
        _ => return None,
    };
    let fallthrough = branch_ip + 1;
    let return_constant = |index: usize| match operations.get(index).copied()? {
        ObjectLongOp::Return {
            value: ObjectLongSource::Constant(value),
        } => Some(value),
        _ => None,
    };
    let select_return = |flag: i64| {
        let condition = flag != 0;
        let index = if condition == jump_when_true {
            return_target
        } else {
            fallthrough
        };
        return_constant(index)
    };
    let when_match = select_return(match_flag)?;
    let when_miss = select_return(miss_flag)?;
    let last_return = return_target.max(fallthrough);
    if operations
        .get(last_return + 1..)?
        .iter()
        .any(|operation| !matches!(operation, ObjectLongOp::Noop | ObjectLongOp::Bail))
    {
        return None;
    }

    Some(Box::new(ObjectLongModuloAnySelect {
        terms: terms.into_boxed_slice(),
        when_match,
        when_miss,
    }))
}

/// Recognize a typed weighted score whose category and threshold branches
/// only add constants to one accumulator. This is a high-level policy shape,
/// not a source-name match: every operand, edge, and checked arithmetic step
/// is proven from the canonical ObjectLong program.
fn build_object_long_weighted_string_score(
    operations: &[ObjectLongOp],
    first_argument: u32,
    argument_end: u32,
) -> Option<Box<ObjectLongWeightedStringScore>> {
    if operations.len() < 7 {
        return None;
    }
    let direct_source = |source: ObjectLongSource| match source {
        ObjectLongSource::Constant(_) => true,
        ObjectLongSource::Slot(slot) => {
            let slot = u32::from(slot);
            slot >= first_argument && slot < argument_end
        }
    };

    let ObjectLongOp::Arithmetic {
        kind: ScalarLongOpKind::Multiply,
        lhs: multiply_lhs,
        rhs: multiply_rhs,
        destination: multiply_result,
    } = operations[0]
    else {
        return None;
    };
    let (weighted_input, multiplier) = match (multiply_lhs, multiply_rhs) {
        (input, ObjectLongSource::Constant(multiplier)) if direct_source(input) => {
            (input, multiplier)
        }
        (ObjectLongSource::Constant(multiplier), input) if direct_source(input) => {
            (input, multiplier)
        }
        _ => return None,
    };

    let ObjectLongOp::Arithmetic {
        kind: ScalarLongOpKind::Add,
        lhs: base_lhs,
        rhs: base_rhs,
        destination: base_sum,
    } = operations[1]
    else {
        return None;
    };
    let additive_input = match (base_lhs, base_rhs) {
        (ObjectLongSource::Slot(slot), input)
            if slot == multiply_result && direct_source(input) =>
        {
            input
        }
        (input, ObjectLongSource::Slot(slot))
            if slot == multiply_result && direct_source(input) =>
        {
            input
        }
        _ => return None,
    };
    let ObjectLongOp::IntDiv {
        lhs: ObjectLongSource::Slot(dividend),
        rhs: ObjectLongSource::Constant(divisor),
        destination: quotient,
    } = operations[2]
    else {
        return None;
    };
    if dividend != base_sum {
        return None;
    }
    let ObjectLongOp::StringLength {
        argument: string_argument,
        destination: string_length,
    } = operations[3]
    else {
        return None;
    };
    let ObjectLongOp::Arithmetic {
        kind: ScalarLongOpKind::Add,
        lhs: score_lhs,
        rhs: score_rhs,
        destination: score_result,
    } = operations[4]
    else {
        return None;
    };
    if !matches!(
        (score_lhs, score_rhs),
        (ObjectLongSource::Slot(lhs), ObjectLongSource::Slot(rhs))
            if (lhs == quotient && rhs == string_length)
                || (lhs == string_length && rhs == quotient)
    ) {
        return None;
    }
    let ObjectLongOp::Assign {
        destination: accumulator,
        source: ObjectLongSource::Slot(source),
    } = operations[5]
    else {
        return None;
    };
    if source != score_result {
        return None;
    }

    let mut string_adjustments = Vec::new();
    let mut string_end_target = None;
    let mut ip = 6usize;
    // A compound scalar RHS may leave canonical intermediate TMPs for the
    // statement cleanup. The ObjectLong program represents that cleanup as a
    // Noop because its own checked operations never materialize those Values.
    while matches!(operations.get(ip), Some(ObjectLongOp::Noop)) {
        ip += 1;
    }
    while string_adjustments.len() < 8 {
        let Some(ObjectLongOp::StringLiteralBranch {
            argument,
            literal,
            jump_when_equal: false,
            target,
        }) = operations.get(ip).copied()
        else {
            break;
        };
        if argument != string_argument
            || !matches!(operations.get(ip + 1), Some(ObjectLongOp::Noop))
        {
            return None;
        }
        let Some(ObjectLongOp::Arithmetic {
            kind: ScalarLongOpKind::Add,
            lhs,
            rhs,
            destination: adjusted,
        }) = operations.get(ip + 2).copied()
        else {
            return None;
        };
        let addend = match (lhs, rhs) {
            (ObjectLongSource::Slot(slot), ObjectLongSource::Constant(addend))
                if slot == accumulator =>
            {
                addend
            }
            (ObjectLongSource::Constant(addend), ObjectLongSource::Slot(slot))
                if slot == accumulator =>
            {
                addend
            }
            _ => return None,
        };
        if !matches!(
            operations.get(ip + 3),
            Some(ObjectLongOp::Assign {
                destination,
                source: ObjectLongSource::Slot(source),
            }) if *destination == accumulator && *source == adjusted
        ) {
            return None;
        }
        string_adjustments.push(ObjectLongStringAdjustment { literal, addend });

        let after_body = ip + 4;
        if let Some(ObjectLongOp::Jump { target: end_target }) = operations.get(after_body).copied()
        {
            if target as usize != after_body + 1
                || string_end_target.is_some_and(|target| target != end_target)
            {
                return None;
            }
            string_end_target = Some(end_target);
            ip = after_body + 1;
        } else {
            if target as usize != after_body {
                return None;
            }
            ip = after_body;
            break;
        }
    }
    if string_end_target.is_some_and(|target| target as usize != ip) {
        return None;
    }

    let mut conditional_adjustments = Vec::new();
    while conditional_adjustments.len() < 8 {
        let Some(ObjectLongOp::Compare {
            kind,
            lhs,
            rhs,
            destination: condition,
        }) = operations.get(ip).copied()
        else {
            break;
        };
        if !direct_source(lhs) || !direct_source(rhs) {
            return None;
        }
        if !matches!(
            operations.get(ip + 1),
            Some(ObjectLongOp::JumpIfFalse {
                condition: ObjectLongSource::Slot(slot),
                target,
            }) if *slot == condition && *target as usize == ip + 4
        ) {
            return None;
        }
        let Some(ObjectLongOp::Arithmetic {
            kind: ScalarLongOpKind::Add,
            lhs: add_lhs,
            rhs: add_rhs,
            destination: adjusted,
        }) = operations.get(ip + 2).copied()
        else {
            return None;
        };
        let addend = match (add_lhs, add_rhs) {
            (ObjectLongSource::Slot(slot), ObjectLongSource::Constant(addend))
                if slot == accumulator =>
            {
                addend
            }
            (ObjectLongSource::Constant(addend), ObjectLongSource::Slot(slot))
                if slot == accumulator =>
            {
                addend
            }
            _ => return None,
        };
        if !matches!(
            operations.get(ip + 3),
            Some(ObjectLongOp::Assign {
                destination,
                source: ObjectLongSource::Slot(source),
            }) if *destination == accumulator && *source == adjusted
        ) {
            return None;
        }
        conditional_adjustments.push(ObjectLongConditionalAdjustment {
            kind,
            lhs,
            rhs,
            addend,
        });
        ip += 4;
    }

    if !matches!(
        operations.get(ip),
        Some(ObjectLongOp::Return {
            value: ObjectLongSource::Slot(slot),
        }) if *slot == accumulator
    ) || operations
        .get(ip + 1..)?
        .iter()
        .any(|operation| !matches!(operation, ObjectLongOp::Noop | ObjectLongOp::Bail))
    {
        return None;
    }

    Some(Box::new(ObjectLongWeightedStringScore {
        weighted_input,
        multiplier,
        additive_input,
        divisor,
        string_argument,
        string_adjustments: string_adjustments.into_boxed_slice(),
        conditional_adjustments: conditional_adjustments.into_boxed_slice(),
    }))
}

/// Recognize a small, side-effect-free method program that reads declared
/// properties from its receiver or positional object arguments and otherwise
/// stays in checked Long operations. Keeping one plan operation per canonical
/// instruction makes forward branches exact and leaves every unsupported edge
/// on the ordinary PHP executor.
fn build_object_long_function_plan(function: &UserFunction) -> Option<Box<ObjectLongFunctionPlan>> {
    let common = &function.common;
    let op_array = &function.op_array;
    let public_args = common.sig.public_arity();
    let slot_count = op_array.num_cvs.checked_add(op_array.num_temps)?;
    if common.sig.this_offset != 1
        || common.sig.returns_reference
        || !common.plan.call.is_compact_user_call()
        || common.plan.ret != ReturnStrategy::Fast
        || common.sig.is_variadic
        || common.sig.ref_args != 0
        || public_args != common.sig.required_num_args
        || public_args > OBJECT_LONG_PLAN_MAX_ARGS
        || slot_count > OBJECT_LONG_PLAN_MAX_SLOTS
        || op_array.instructions.len() > OBJECT_LONG_PLAN_MAX_OPS
        || op_array.instructions.len() > u16::MAX as usize
        || !matches!(
            common.sig.return_type_hint,
            ParamTypeHint::None | ParamTypeHint::Mixed | ParamTypeHint::Int
        )
        || common.sig.param_type_hints.iter().any(|hint| {
            !matches!(
                hint,
                ParamTypeHint::None
                    | ParamTypeHint::Mixed
                    | ParamTypeHint::Int
                    | ParamTypeHint::String
                    | ParamTypeHint::ClassName(_)
            )
        })
    {
        return None;
    }

    let mut initialized = [false; OBJECT_LONG_PLAN_MAX_SLOTS as usize];
    let mut long_argument_mask = 0u8;
    let mut object_argument_mask = 0u8;
    let mut string_argument_mask = 0u8;
    let mut operations = Vec::with_capacity(op_array.instructions.len());
    let first_argument = common.sig.this_offset;
    let argument_end = first_argument + public_args;
    let mut dead_fused_branch = None;

    for (ip, instruction) in op_array.instructions.iter().enumerate() {
        if dead_fused_branch == Some(ip) {
            if !matches!(instruction.opcode, OpCode::JmpZ | OpCode::JmpNZ) {
                return None;
            }
            operations.push(ObjectLongOp::Noop);
            dead_fused_branch = None;
            continue;
        }
        let operation = match instruction.opcode {
            OpCode::AssignCv => {
                if !matches!(instruction.op1_type, OpType::Cv | OpType::Tmp | OpType::Var) {
                    return None;
                }
                let destination = instruction.op1 as u32;
                // Rebinding `$this` or a public input would invalidate the
                // adapter-owned object/Long bindings.
                if instruction.op1_type == OpType::Cv && destination < argument_end {
                    return None;
                }
                let source = if instruction.op1_type != OpType::Cv
                    && instruction.op2_type == OpType::Const
                {
                    match op_array
                        .literals
                        .get(instruction.op2 as usize)?
                        .value_type()
                    {
                        crate::value::ValueType::False => ObjectLongSource::Constant(0),
                        crate::value::ValueType::True => ObjectLongSource::Constant(1),
                        _ => object_long_source(
                            function,
                            &initialized,
                            &mut long_argument_mask,
                            instruction.op2_type,
                            instruction.op2,
                        )?,
                    }
                } else {
                    object_long_source(
                        function,
                        &initialized,
                        &mut long_argument_mask,
                        instruction.op2_type,
                        instruction.op2,
                    )?
                };
                initialized[destination as usize] = true;
                ObjectLongOp::Assign {
                    destination: instruction.op1,
                    source,
                }
            }
            OpCode::FetchObjR => {
                if instruction.op1_type != OpType::Cv
                    || instruction.op2_type != OpType::Const
                    || !matches!(instruction.result_type, OpType::Tmp | OpType::Var)
                    || op_array
                        .literals
                        .get(instruction.op2 as usize)
                        .and_then(Value::as_str)
                        .is_none()
                {
                    return None;
                }
                let object = if instruction.op1 == 0 {
                    ObjectLongObjectSource::Receiver
                } else {
                    let slot = instruction.op1 as u32;
                    if slot < first_argument || slot >= argument_end {
                        return None;
                    }
                    let argument = (slot - first_argument) as u8;
                    object_argument_mask |= 1 << argument;
                    ObjectLongObjectSource::Argument(argument)
                };
                initialized[instruction.result as usize] = true;
                ObjectLongOp::FetchProperty {
                    object,
                    cache_ip: ip as u16,
                    destination: instruction.result,
                }
            }
            opcode if scalar_long_op_kind(opcode).is_some() => {
                if !matches!(instruction.result_type, OpType::Tmp | OpType::Var) {
                    return None;
                }
                let lhs = object_long_source(
                    function,
                    &initialized,
                    &mut long_argument_mask,
                    instruction.op1_type,
                    instruction.op1,
                )?;
                let rhs = object_long_source(
                    function,
                    &initialized,
                    &mut long_argument_mask,
                    instruction.op2_type,
                    instruction.op2,
                )?;
                initialized[instruction.result as usize] = true;
                ObjectLongOp::Arithmetic {
                    kind: scalar_long_op_kind(opcode)?,
                    lhs,
                    rhs,
                    destination: instruction.result,
                }
            }
            OpCode::IsEqual | OpCode::IsIdentical => {
                let lhs = object_long_source(
                    function,
                    &initialized,
                    &mut long_argument_mask,
                    instruction.op1_type,
                    instruction.op1,
                )?;
                let rhs = object_long_source(
                    function,
                    &initialized,
                    &mut long_argument_mask,
                    instruction.op2_type,
                    instruction.op2,
                )?;
                initialized[instruction.result as usize] = true;
                ObjectLongOp::Compare {
                    kind: ScalarLongConditionKind::Equal,
                    lhs,
                    rhs,
                    destination: instruction.result,
                }
            }
            OpCode::IsNotEqual | OpCode::IsNotIdentical => {
                let lhs = object_long_source(
                    function,
                    &initialized,
                    &mut long_argument_mask,
                    instruction.op1_type,
                    instruction.op1,
                )?;
                let rhs = object_long_source(
                    function,
                    &initialized,
                    &mut long_argument_mask,
                    instruction.op2_type,
                    instruction.op2,
                )?;
                initialized[instruction.result as usize] = true;
                ObjectLongOp::Compare {
                    kind: ScalarLongConditionKind::NotEqual,
                    lhs,
                    rhs,
                    destination: instruction.result,
                }
            }
            OpCode::IsSmaller => {
                let lhs = object_long_source(
                    function,
                    &initialized,
                    &mut long_argument_mask,
                    instruction.op1_type,
                    instruction.op1,
                )?;
                let rhs = object_long_source(
                    function,
                    &initialized,
                    &mut long_argument_mask,
                    instruction.op2_type,
                    instruction.op2,
                )?;
                initialized[instruction.result as usize] = true;
                ObjectLongOp::Compare {
                    kind: ScalarLongConditionKind::LessThan,
                    lhs,
                    rhs,
                    destination: instruction.result,
                }
            }
            OpCode::IsSmallerOrEqual => {
                let lhs = object_long_source(
                    function,
                    &initialized,
                    &mut long_argument_mask,
                    instruction.op1_type,
                    instruction.op1,
                )?;
                let rhs = object_long_source(
                    function,
                    &initialized,
                    &mut long_argument_mask,
                    instruction.op2_type,
                    instruction.op2,
                )?;
                initialized[instruction.result as usize] = true;
                ObjectLongOp::Compare {
                    kind: ScalarLongConditionKind::LessThanOrEqual,
                    lhs,
                    rhs,
                    destination: instruction.result,
                }
            }
            OpCode::JmpZ_Eq_CvConst | OpCode::JmpNZ_Eq_CvConst => {
                if instruction.op1_type != OpType::Cv || instruction.op2_type != OpType::Const {
                    return None;
                }
                let slot = instruction.op1 as u32;
                if slot < first_argument || slot >= argument_end {
                    return None;
                }
                let argument = (slot - first_argument) as u8;
                if op_array
                    .literals
                    .get(instruction.op2 as usize)
                    .and_then(Value::as_str)
                    .is_none()
                {
                    return None;
                }
                let target = instruction.result as usize;
                if target <= ip || target >= op_array.instructions.len() {
                    return None;
                }
                string_argument_mask |= 1 << argument;
                dead_fused_branch = Some(ip + 1);
                ObjectLongOp::StringLiteralBranch {
                    argument,
                    literal: instruction.op2,
                    jump_when_equal: instruction.opcode == OpCode::JmpNZ_Eq_CvConst,
                    target: target as u16,
                }
            }
            OpCode::Strlen | OpCode::Strlen_Cv | OpCode::Strlen_String => {
                if instruction.op1_type != OpType::Cv
                    || !matches!(instruction.result_type, OpType::Tmp | OpType::Var)
                {
                    return None;
                }
                let slot = instruction.op1 as u32;
                if slot < first_argument || slot >= argument_end {
                    return None;
                }
                let argument = (slot - first_argument) as u8;
                string_argument_mask |= 1 << argument;
                initialized[instruction.result as usize] = true;
                ObjectLongOp::StringLength {
                    argument,
                    destination: instruction.result,
                }
            }
            OpCode::DirectInternalCall2 => {
                if crate::builtin_metadata::DirectInternalKind::from_id(instruction.extended_value)
                    != Some(crate::builtin_metadata::DirectInternalKind::Intdiv)
                    || !matches!(instruction.result_type, OpType::Tmp | OpType::Var)
                {
                    return None;
                }
                let lhs = object_long_source(
                    function,
                    &initialized,
                    &mut long_argument_mask,
                    instruction.op1_type,
                    instruction.op1,
                )?;
                let rhs = object_long_source(
                    function,
                    &initialized,
                    &mut long_argument_mask,
                    instruction.op2_type,
                    instruction.op2,
                )?;
                initialized[instruction.result as usize] = true;
                ObjectLongOp::IntDiv {
                    lhs,
                    rhs,
                    destination: instruction.result,
                }
            }
            OpCode::JmpZ | OpCode::JmpNZ => {
                let target = instruction.op2 as usize;
                if target <= ip || target >= op_array.instructions.len() {
                    return None;
                }
                let condition = object_long_source(
                    function,
                    &initialized,
                    &mut long_argument_mask,
                    instruction.op1_type,
                    instruction.op1,
                )?;
                if instruction.opcode == OpCode::JmpZ {
                    ObjectLongOp::JumpIfFalse {
                        condition,
                        target: target as u16,
                    }
                } else {
                    ObjectLongOp::JumpIfTrue {
                        condition,
                        target: target as u16,
                    }
                }
            }
            OpCode::Jmp => {
                let target = instruction.op1 as usize;
                if target <= ip || target >= op_array.instructions.len() {
                    return None;
                }
                ObjectLongOp::Jump {
                    target: target as u16,
                }
            }
            OpCode::Return if instruction.extended_value != 0 => ObjectLongOp::Return {
                value: object_long_source(
                    function,
                    &initialized,
                    &mut long_argument_mask,
                    instruction.op1_type,
                    instruction.op1,
                )?,
            },
            OpCode::Return => ObjectLongOp::Bail,
            // The object/Long plan computes into its own scalar slots and
            // never materializes canonical TMP/VAR owners. A cleanup emitted
            // after assignment is therefore a one-for-one control-flow no-op.
            OpCode::ReleaseTemps => ObjectLongOp::Noop,
            _ => return None,
        };
        operations.push(operation);
    }

    if operations.is_empty()
        || long_argument_mask & (object_argument_mask | string_argument_mask) != 0
        || object_argument_mask & string_argument_mask != 0
        || !operations
            .iter()
            .any(|operation| matches!(operation, ObjectLongOp::Return { .. }))
    {
        return None;
    }

    let string_intdiv_select = build_object_long_string_intdiv_select(&operations);
    let modulo_any_select = build_object_long_modulo_any_select(&operations);
    let weighted_string_score =
        build_object_long_weighted_string_score(&operations, first_argument, argument_end);
    Some(Box::new(ObjectLongFunctionPlan {
        public_args: public_args as u8,
        long_argument_mask,
        object_argument_mask,
        string_argument_mask,
        slot_count: slot_count as u16,
        operations: operations.into_boxed_slice(),
        string_intdiv_select,
        modulo_any_select,
        weighted_string_score,
    }))
}

fn object_array_source(
    function: &UserFunction,
    aliases: &[Option<ObjectArraySource>; OBJECT_LONG_PLAN_MAX_SLOTS as usize],
    initialized_long: &[bool; OBJECT_LONG_PLAN_MAX_SLOTS as usize],
    op_type: OpType,
    operand: u16,
) -> Option<ObjectArraySource> {
    let common = &function.common;
    match op_type {
        OpType::Const => function
            .op_array
            .literals
            .get(operand as usize)
            .map(|_| ObjectArraySource::Literal(operand)),
        OpType::Cv => {
            let slot = operand as u32;
            if slot == 0 {
                return Some(ObjectArraySource::Receiver);
            }
            let first_argument = common.sig.this_offset;
            let argument_end = first_argument + common.sig.public_arity();
            if slot >= first_argument && slot < argument_end {
                Some(ObjectArraySource::Argument((slot - first_argument) as u8))
            } else if initialized_long
                .get(slot as usize)
                .copied()
                .unwrap_or(false)
            {
                Some(ObjectArraySource::LongSlot(operand))
            } else {
                aliases.get(slot as usize).copied().flatten()
            }
        }
        OpType::Tmp | OpType::Var => {
            aliases
                .get(operand as usize)
                .copied()
                .flatten()
                .or_else(|| {
                    initialized_long
                        .get(operand as usize)
                        .copied()
                        .filter(|initialized| *initialized)
                        .map(|_| ObjectArraySource::LongSlot(operand))
                })
        }
        OpType::Unused => None,
    }
}

struct PendingObjectArrayCall {
    cache_ip: u16,
    receiver: ObjectArraySource,
    expected_arguments: usize,
    arguments: Vec<ObjectArraySource>,
}

/// Recognize a compact read-only application method which composes existing
/// object/Long callees and returns a small literal-key array. Unlike a
/// benchmark-specific superinstruction, the plan is expressed only in terms
/// of PHP data dependencies and the canonical inline-cache positions.
fn build_object_array_function_plan(
    function: &UserFunction,
) -> Option<Box<ObjectArrayFunctionPlan>> {
    const MAX_ENTRIES: usize = 4;

    let common = &function.common;
    let op_array = &function.op_array;
    let public_args = common.sig.public_arity();
    let slot_count = op_array.num_cvs.checked_add(op_array.num_temps)?;
    if common.sig.this_offset != 1
        || common.sig.returns_reference
        || !common.plan.call.is_compact_user_call()
        || common.plan.ret != ReturnStrategy::Fast
        || common.sig.is_variadic
        || common.sig.ref_args != 0
        || public_args != common.sig.required_num_args
        || public_args > OBJECT_LONG_PLAN_MAX_ARGS
        || slot_count > OBJECT_LONG_PLAN_MAX_SLOTS
        || op_array.instructions.len() > OBJECT_LONG_PLAN_MAX_OPS
        || op_array.instructions.len() > u16::MAX as usize
        || !matches!(
            common.sig.return_type_hint,
            ParamTypeHint::None | ParamTypeHint::Mixed | ParamTypeHint::Array
        )
        || common.sig.param_type_hints.iter().any(|hint| {
            !matches!(
                hint,
                ParamTypeHint::None
                    | ParamTypeHint::Mixed
                    | ParamTypeHint::Int
                    | ParamTypeHint::String
                    | ParamTypeHint::ClassName(_)
            )
        })
    {
        return None;
    }

    let mut aliases = [None; OBJECT_LONG_PLAN_MAX_SLOTS as usize];
    let mut initialized_long = [false; OBJECT_LONG_PLAN_MAX_SLOTS as usize];
    let mut pending_call: Option<PendingObjectArrayCall> = None;
    let mut operations = Vec::new();
    let mut entries = Vec::new();
    let mut array_slot = None;
    let mut returned = false;
    let first_argument = common.sig.this_offset;
    let argument_end = first_argument + public_args;

    for (ip, instruction) in op_array.instructions.iter().enumerate() {
        if returned {
            if instruction.opcode == OpCode::Return && instruction.extended_value == 0 {
                continue;
            }
            return None;
        }

        match instruction.opcode {
            OpCode::FetchObjR => {
                if instruction.op2_type != OpType::Const
                    || !matches!(instruction.result_type, OpType::Tmp | OpType::Var)
                    || op_array
                        .literals
                        .get(instruction.op2 as usize)
                        .and_then(Value::as_str)
                        .is_none()
                {
                    return None;
                }
                let object = match object_array_source(
                    function,
                    &aliases,
                    &initialized_long,
                    instruction.op1_type,
                    instruction.op1,
                )? {
                    ObjectArraySource::Receiver => ObjectLongObjectSource::Receiver,
                    ObjectArraySource::Argument(argument) => {
                        ObjectLongObjectSource::Argument(argument)
                    }
                    _ => return None,
                };
                aliases[instruction.result as usize] = Some(ObjectArraySource::Property {
                    object,
                    cache_ip: ip as u16,
                });
                initialized_long[instruction.result as usize] = false;
            }
            OpCode::InitMethodCall => {
                if pending_call.is_some()
                    || instruction.op2_type != OpType::Const
                    || op_array
                        .literals
                        .get(instruction.op2 as usize)
                        .and_then(Value::as_str)
                        .is_none()
                    || instruction.extended_value as usize > OBJECT_LONG_PLAN_MAX_ARGS as usize
                {
                    return None;
                }
                pending_call = Some(PendingObjectArrayCall {
                    cache_ip: ip as u16,
                    receiver: object_array_source(
                        function,
                        &aliases,
                        &initialized_long,
                        instruction.op1_type,
                        instruction.op1,
                    )?,
                    expected_arguments: instruction.extended_value as usize,
                    arguments: Vec::with_capacity(instruction.extended_value as usize),
                });
            }
            OpCode::SendVal | OpCode::SendVarEx => {
                let argument = object_array_source(
                    function,
                    &aliases,
                    &initialized_long,
                    instruction.op1_type,
                    instruction.op1,
                )?;
                let pending = pending_call.as_mut()?;
                if pending.arguments.len() == pending.expected_arguments {
                    return None;
                }
                pending.arguments.push(argument);
            }
            OpCode::DoFcall => {
                if !matches!(instruction.result_type, OpType::Tmp | OpType::Var) {
                    return None;
                }
                let pending = pending_call.take()?;
                if pending.arguments.len() != pending.expected_arguments {
                    return None;
                }
                aliases[instruction.result as usize] = None;
                initialized_long[instruction.result as usize] = true;
                operations.push(ObjectArrayLongOp::Call(ObjectArrayLongCall {
                    cache_ip: pending.cache_ip,
                    receiver: pending.receiver,
                    arguments: pending.arguments.into_boxed_slice(),
                    destination: instruction.result,
                }));
            }
            OpCode::ReleaseTemps => {
                // Receivers and scalar results represented virtually by this
                // plan still have an exact baseline lifetime range. Consume
                // that bounded range in the proof state; the plan itself does
                // not materialize any of these canonical Value slots.
                if pending_call.is_some()
                    || instruction.op1_type != OpType::Tmp
                    || instruction.op2_type != OpType::Tmp
                    || instruction.op1 >= instruction.op2
                {
                    return None;
                }
                let start = instruction.op1 as usize;
                let end = instruction.op2 as usize;
                if end > aliases.len() {
                    return None;
                }
                for slot in start..end {
                    aliases[slot] = None;
                    initialized_long[slot] = false;
                }
            }
            OpCode::AssignCv => {
                if pending_call.is_some() || instruction.op1_type != OpType::Cv {
                    return None;
                }
                let destination = instruction.op1 as u32;
                if destination < argument_end || destination >= slot_count {
                    return None;
                }
                let source = object_array_source(
                    function,
                    &aliases,
                    &initialized_long,
                    instruction.op2_type,
                    instruction.op2,
                )?;
                aliases[destination as usize] = None;
                initialized_long[destination as usize] = true;
                operations.push(ObjectArrayLongOp::Assign {
                    destination: instruction.op1,
                    source,
                });
            }
            opcode if scalar_long_op_kind(opcode).is_some() => {
                if pending_call.is_some()
                    || !matches!(instruction.result_type, OpType::Tmp | OpType::Var)
                {
                    return None;
                }
                let lhs = object_array_source(
                    function,
                    &aliases,
                    &initialized_long,
                    instruction.op1_type,
                    instruction.op1,
                )?;
                let rhs = object_array_source(
                    function,
                    &aliases,
                    &initialized_long,
                    instruction.op2_type,
                    instruction.op2,
                )?;
                aliases[instruction.result as usize] = None;
                initialized_long[instruction.result as usize] = true;
                operations.push(ObjectArrayLongOp::Arithmetic {
                    kind: scalar_long_op_kind(opcode)?,
                    lhs,
                    rhs,
                    destination: instruction.result,
                });
            }
            OpCode::DirectInternalCall2 => {
                if pending_call.is_some()
                    || crate::builtin_metadata::DirectInternalKind::from_id(
                        instruction.extended_value,
                    ) != Some(crate::builtin_metadata::DirectInternalKind::Intdiv)
                    || !matches!(instruction.result_type, OpType::Tmp | OpType::Var)
                {
                    return None;
                }
                let lhs = object_array_source(
                    function,
                    &aliases,
                    &initialized_long,
                    instruction.op1_type,
                    instruction.op1,
                )?;
                let rhs = object_array_source(
                    function,
                    &aliases,
                    &initialized_long,
                    instruction.op2_type,
                    instruction.op2,
                )?;
                aliases[instruction.result as usize] = None;
                initialized_long[instruction.result as usize] = true;
                operations.push(ObjectArrayLongOp::IntDiv {
                    lhs,
                    rhs,
                    destination: instruction.result,
                });
            }
            OpCode::InitArray => {
                if pending_call.is_some()
                    || array_slot.is_some()
                    || !matches!(instruction.result_type, OpType::Tmp | OpType::Var)
                    || instruction.extended_value == 0
                    || instruction.extended_value as usize > MAX_ENTRIES
                    || instruction._pad & crate::vm::instruction::ARRAY_INIT_HASH_HINT == 0
                {
                    return None;
                }
                array_slot = Some((instruction.result_type, instruction.result));
            }
            OpCode::AddArrayElement => {
                if pending_call.is_some()
                    || Some((instruction.op1_type, instruction.op1)) != array_slot
                    || instruction._pad & crate::vm::instruction::ARRAY_ELEMENT_REFERENCE != 0
                    || instruction.result_type != OpType::Const
                    || entries.len() == MAX_ENTRIES
                    || op_array
                        .literals
                        .get(instruction.result as usize)
                        .and_then(Value::as_str)
                        .is_none()
                {
                    return None;
                }
                entries.push(ObjectArrayEntry {
                    key_literal: instruction.result,
                    value: object_array_source(
                        function,
                        &aliases,
                        &initialized_long,
                        instruction.op2_type,
                        instruction.op2,
                    )?,
                });
            }
            OpCode::Return if instruction.extended_value != 0 => {
                if pending_call.is_some()
                    || Some((instruction.op1_type, instruction.op1)) != array_slot
                {
                    return None;
                }
                returned = true;
            }
            OpCode::Return => return None,
            _ => return None,
        }
    }

    if !returned
        || pending_call.is_some()
        || entries.is_empty()
        || entries.len() > MAX_ENTRIES
        || operations.is_empty()
        || !operations
            .iter()
            .any(|operation| matches!(operation, ObjectArrayLongOp::Call(_)))
    {
        return None;
    }

    Some(Box::new(ObjectArrayFunctionPlan {
        public_args: public_args as u8,
        slot_count: slot_count as u16,
        operations: operations.into_boxed_slice(),
        entries: entries.into_boxed_slice(),
    }))
}

fn scalar_string_return_literal(
    function: &UserFunction,
    start: usize,
    limit: usize,
) -> Option<Box<str>> {
    let instructions = function.op_array.instructions.get(start..limit)?;
    if instructions.is_empty() {
        return None;
    }
    let instruction = instructions.first()?;
    if instruction.opcode != OpCode::Return
        || instruction.extended_value == 0
        || instruction.op1_type != OpType::Const
    {
        return None;
    }
    function
        .op_array
        .literals
        .get(instruction.op1 as usize)?
        .as_str()
        .map(Box::<str>::from)
}

/// Recognize a pure function whose result is an immutable string literal,
/// optionally selected by the same guarded Long predicates used by scalar
/// integer plans. The owned plan strings provide a stable borrowed result for
/// typed consumers without materializing a PHP Value.
pub(crate) fn build_scalar_string_function_plan(
    function: &UserFunction,
) -> Option<Box<ScalarStringFunctionPlan>> {
    let common = &function.common;
    let instructions = &function.op_array.instructions;
    let public_args = common.sig.public_arity();
    if common.sig.is_variadic
        || common.sig.returns_reference
        || common.sig.ref_args != 0
        || public_args > SCALAR_LONG_PLAN_MAX_ARGS
        || common.plan.ret != ReturnStrategy::Fast
        || instructions.len() > 32
        || !common.sig.param_type_hints.iter().all(|hint| {
            matches!(
                hint,
                ParamTypeHint::None | ParamTypeHint::Mixed | ParamTypeHint::Int
            )
        })
        || !matches!(
            common.sig.return_type_hint,
            ParamTypeHint::None | ParamTypeHint::Mixed | ParamTypeHint::String
        )
    {
        return None;
    }

    if let Some(scope_tmp) = function.op_array.trait_class_scope_tmp
        && let Some(first) = instructions.first()
        && first.opcode == OpCode::Return
        && first.extended_value != 0
        && first.op1_type == OpType::Tmp
        && first.op1 == scope_tmp
        && instructions[1..]
            .iter()
            .all(|instruction| instruction.opcode == OpCode::Return)
    {
        return Some(Box::new(ScalarStringFunctionPlan {
            public_args: public_args as u8,
            operations: Box::new([]),
            select: None,
            when_false: Box::from(""),
            when_true: Box::from(""),
            trait_class_scope: true,
        }));
    }

    if let Some(value) = scalar_string_return_literal(function, 0, instructions.len()) {
        return Some(Box::new(ScalarStringFunctionPlan {
            public_args: public_args as u8,
            operations: Box::new([]),
            select: None,
            when_false: value.clone(),
            when_true: value,
            trait_class_scope: false,
        }));
    }

    let mut temporary_results = HashMap::new();
    let mut masked_results = HashMap::new();
    let mut operations = Vec::new();
    let mut ip = 0usize;
    while let Some(instruction) = instructions.get(ip) {
        if instruction.opcode == OpCode::ReleaseTemps {
            ip += 1;
            continue;
        }
        if instruction.opcode == OpCode::BitwiseAnd {
            if !matches!(instruction.result_type, OpType::Tmp | OpType::Var) {
                return None;
            }
            let lhs = scalar_long_condition_operand(
                function,
                &temporary_results,
                &masked_results,
                instruction.op1_type,
                instruction.op1,
            )?;
            let rhs = scalar_long_condition_operand(
                function,
                &temporary_results,
                &masked_results,
                instruction.op2_type,
                instruction.op2,
            )?;
            let ScalarLongConditionOperand::Source(lhs) = lhs else {
                return None;
            };
            let ScalarLongConditionOperand::Source(rhs) = rhs else {
                return None;
            };
            masked_results.insert(
                instruction.result,
                ScalarLongConditionOperand::BitwiseAnd { lhs, rhs },
            );
            ip += 1;
            continue;
        }
        if scalar_long_op_kind(instruction.opcode).is_some() {
            append_scalar_long_operation(
                function,
                instruction,
                &mut temporary_results,
                &mut operations,
            )?;
            ip += 1;
            continue;
        }
        if instruction.opcode == OpCode::AssignCv {
            bind_scalar_long_local(function, instruction, &mut temporary_results)?;
            ip += 1;
            continue;
        }
        break;
    }
    if operations.len() > SCALAR_LONG_PLAN_MAX_OPS {
        return None;
    }

    let condition_instruction = *instructions.get(ip)?;
    let (kind, lhs, rhs, branch_ip, fused_jump_target) = match condition_instruction.opcode {
        OpCode::IsEqual | OpCode::IsIdentical | OpCode::IsEqual_CvConst => (
            ScalarLongConditionKind::Equal,
            scalar_long_condition_operand(
                function,
                &temporary_results,
                &masked_results,
                condition_instruction.op1_type,
                condition_instruction.op1,
            )?,
            scalar_long_condition_operand(
                function,
                &temporary_results,
                &masked_results,
                condition_instruction.op2_type,
                condition_instruction.op2,
            )?,
            ip + 1,
            None,
        ),
        OpCode::IsNotEqual | OpCode::IsNotIdentical => (
            ScalarLongConditionKind::NotEqual,
            scalar_long_condition_operand(
                function,
                &temporary_results,
                &masked_results,
                condition_instruction.op1_type,
                condition_instruction.op1,
            )?,
            scalar_long_condition_operand(
                function,
                &temporary_results,
                &masked_results,
                condition_instruction.op2_type,
                condition_instruction.op2,
            )?,
            ip + 1,
            None,
        ),
        OpCode::IsSmaller | OpCode::IsSmaller_CvConst => (
            ScalarLongConditionKind::LessThan,
            scalar_long_condition_operand(
                function,
                &temporary_results,
                &masked_results,
                condition_instruction.op1_type,
                condition_instruction.op1,
            )?,
            scalar_long_condition_operand(
                function,
                &temporary_results,
                &masked_results,
                condition_instruction.op2_type,
                condition_instruction.op2,
            )?,
            ip + 1,
            None,
        ),
        OpCode::IsSmallerOrEqual | OpCode::IsSmallerOrEqual_CvConst => (
            ScalarLongConditionKind::LessThanOrEqual,
            scalar_long_condition_operand(
                function,
                &temporary_results,
                &masked_results,
                condition_instruction.op1_type,
                condition_instruction.op1,
            )?,
            scalar_long_condition_operand(
                function,
                &temporary_results,
                &masked_results,
                condition_instruction.op2_type,
                condition_instruction.op2,
            )?,
            ip + 1,
            None,
        ),
        OpCode::JmpZ => (
            ScalarLongConditionKind::NotEqual,
            scalar_long_condition_operand(
                function,
                &temporary_results,
                &masked_results,
                condition_instruction.op1_type,
                condition_instruction.op1,
            )?,
            ScalarLongConditionOperand::Source(ScalarLongSource::Constant(0)),
            ip,
            None,
        ),
        OpCode::JmpZ_Eq_CvConst | OpCode::JmpZ_Lt_CvConst | OpCode::JmpZ_Le_CvConst => (
            match condition_instruction.opcode {
                OpCode::JmpZ_Eq_CvConst => ScalarLongConditionKind::Equal,
                OpCode::JmpZ_Lt_CvConst => ScalarLongConditionKind::LessThan,
                OpCode::JmpZ_Le_CvConst => ScalarLongConditionKind::LessThanOrEqual,
                _ => unreachable!(),
            },
            scalar_long_condition_operand(
                function,
                &temporary_results,
                &masked_results,
                condition_instruction.op1_type,
                condition_instruction.op1,
            )?,
            scalar_long_condition_operand(
                function,
                &temporary_results,
                &masked_results,
                condition_instruction.op2_type,
                condition_instruction.op2,
            )?,
            ip,
            Some(condition_instruction.result as usize),
        ),
        _ => return None,
    };

    let (when_true_ip, when_false_ip) = if let Some(target) = fused_jump_target {
        (ip + 2, target)
    } else {
        let branch = instructions.get(branch_ip)?;
        if branch.opcode != OpCode::JmpZ {
            return None;
        }
        if branch_ip != ip
            && (!matches!(condition_instruction.result_type, OpType::Tmp | OpType::Var)
                || branch.op1_type != condition_instruction.result_type
                || branch.op1 != condition_instruction.result)
        {
            return None;
        }
        (branch_ip + 1, branch.op2 as usize)
    };
    if when_true_ip >= when_false_ip || when_false_ip >= instructions.len() {
        return None;
    }
    let when_true = scalar_string_return_literal(function, when_true_ip, when_false_ip)?;
    let when_false = scalar_string_return_literal(function, when_false_ip, instructions.len())?;

    Some(Box::new(ScalarStringFunctionPlan {
        public_args: public_args as u8,
        operations: operations.into_boxed_slice(),
        select: Some(ScalarStringSelect { kind, lhs, rhs }),
        when_true,
        when_false,
        trait_class_scope: false,
    }))
}

const COMPOSED_SCALAR_LONG_PLAN_MAX_OPS: usize = 16;

fn composed_scalar_argument_masks(function: &UserFunction) -> Option<(u8, u8)> {
    let common = &function.common;
    let public_args = common.sig.public_arity();
    if common.sig.is_variadic
        || common.sig.ref_args != 0
        || public_args > SCALAR_LONG_PLAN_MAX_ARGS
        || !matches!(
            common.sig.return_type_hint,
            ParamTypeHint::None | ParamTypeHint::Mixed | ParamTypeHint::Int
        )
    {
        return None;
    }

    let mut long_mask = 0u8;
    let mut object_mask = 0u8;
    for index in 0..public_args as usize {
        let hint = common
            .sig
            .param_type_hints
            .get(index)
            .unwrap_or(&ParamTypeHint::None);
        match hint {
            ParamTypeHint::None | ParamTypeHint::Mixed | ParamTypeHint::Int => {
                long_mask |= 1u8 << index;
            }
            ParamTypeHint::ClassName(_) => object_mask |= 1u8 << index,
            _ => return None,
        }
    }
    Some((long_mask, object_mask))
}

/// Infer exact borrowed-String inputs only from operations whose semantics
/// require a String. This lets an erased `T`/`mixed` parameter participate in
/// the typed plan without guessing from its broad executable signature; any
/// incompatible numeric use makes the subsequent plan construction fail.
fn composed_typed_string_argument_uses(function: &UserFunction) -> u8 {
    let common = &function.common;
    let public_args = common.sig.public_arity() as usize;
    let argument_for_cv = |cv: u16| {
        (0..public_args).find(|index| common.sig.param_cv_index(*index as u32) == u32::from(cv))
    };
    let mut string_mask = 0u8;
    for instruction in function.op_array.instructions.iter() {
        let string_cv = if matches!(
            instruction.opcode,
            OpCode::Strlen | OpCode::Strlen_Cv | OpCode::Strlen_String
        ) && instruction.op1_type == OpType::Cv
        {
            Some(instruction.op1)
        } else if matches!(
            instruction.opcode,
            OpCode::Concat | OpCode::Concat_StringString
        ) {
            if instruction.op1_type == OpType::Cv && instruction.op2_type == OpType::Const {
                Some(instruction.op1)
            } else if instruction.op2_type == OpType::Cv && instruction.op1_type == OpType::Const {
                Some(instruction.op2)
            } else {
                None
            }
        } else {
            None
        };
        if let Some(index) = string_cv.and_then(argument_for_cv) {
            string_mask |= 1u8 << index;
        }
    }
    string_mask
}

fn composed_typed_argument_masks(function: &UserFunction) -> Option<(u8, u8, u8)> {
    let common = &function.common;
    let public_args = common.sig.public_arity();
    if common.sig.is_variadic
        || common.sig.ref_args != 0
        || public_args > SCALAR_LONG_PLAN_MAX_ARGS
        || !matches!(
            common.sig.return_type_hint,
            ParamTypeHint::None | ParamTypeHint::Mixed | ParamTypeHint::Int
        )
    {
        return None;
    }

    let mut long_mask = 0u8;
    let mut object_mask = 0u8;
    let mut string_mask = 0u8;
    let inferred_string_mask = composed_typed_string_argument_uses(function);
    for index in 0..public_args as usize {
        let hint = common
            .sig
            .param_type_hints
            .get(index)
            .unwrap_or(&ParamTypeHint::None);
        match hint {
            ParamTypeHint::None | ParamTypeHint::Mixed
                if inferred_string_mask & (1u8 << index) != 0 =>
            {
                string_mask |= 1u8 << index;
            }
            ParamTypeHint::None | ParamTypeHint::Mixed | ParamTypeHint::Int => {
                long_mask |= 1u8 << index;
            }
            ParamTypeHint::ClassName(_) => object_mask |= 1u8 << index,
            ParamTypeHint::String => string_mask |= 1u8 << index,
            _ => return None,
        }
    }
    Some((long_mask, object_mask, string_mask))
}

fn composed_scalar_long_source(
    function: &UserFunction,
    long_argument_mask: u8,
    temporary_results: &HashMap<u16, ScalarLongSource>,
    op_type: OpType,
    operand: u16,
) -> Option<ScalarLongSource> {
    let source = scalar_long_source(
        &function.op_array,
        temporary_results,
        function.common.sig.this_offset,
        function.common.sig.public_arity(),
        op_type,
        operand,
    )?;
    match source {
        ScalarLongSource::Input(index) if long_argument_mask & (1u8 << index) == 0 => None,
        _ => Some(source),
    }
}

fn bind_composed_scalar_long_local(
    function: &UserFunction,
    long_argument_mask: u8,
    instruction: &Instruction,
    temporary_results: &mut HashMap<u16, ScalarLongSource>,
) -> Option<()> {
    if instruction.opcode != OpCode::AssignCv || instruction.op1_type != OpType::Cv {
        return None;
    }
    let destination = instruction.op1 as u32;
    let first_argument = function.common.sig.this_offset;
    let argument_end = first_argument + function.common.sig.public_arity();
    if destination < first_argument || (destination >= first_argument && destination < argument_end)
    {
        return None;
    }
    let source = composed_scalar_long_source(
        function,
        long_argument_mask,
        temporary_results,
        instruction.op2_type,
        instruction.op2,
    )?;
    temporary_results.insert(instruction.op1, source);
    Some(())
}

/// Recognize a straight-line integer body that composes direct user functions
/// with arithmetic. This intentionally retains a two-variant operation enum:
/// adding String support must not widen dispatch in the established Long-only
/// executor.
fn build_composed_scalar_long_function_plan(
    function: &UserFunction,
) -> Option<Box<ComposedScalarLongFunctionPlan>> {
    let common = &function.common;
    let op_array = &function.op_array;
    let public_args = common.sig.public_arity();
    let (long_argument_mask, object_argument_mask) = composed_scalar_argument_masks(function)?;
    if common.sig.returns_reference
        || common.plan.ret != ReturnStrategy::Fast
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
        if instruction.opcode == OpCode::ReleaseTemps {
            ip += 1;
            continue;
        }
        if instruction.opcode == OpCode::Return {
            if instruction.extended_value == 0 || !contains_call {
                return None;
            }
            let result = composed_scalar_long_source(
                function,
                long_argument_mask,
                &temporary_results,
                instruction.op1_type,
                instruction.op1,
            )?;
            return Some(Box::new(ComposedScalarLongFunctionPlan {
                public_args: public_args as u8,
                long_argument_mask,
                object_argument_mask,
                program: ScalarLongProgram {
                    operations: operations.into_boxed_slice(),
                    outputs: [result],
                    output_count: 1,
                },
            }));
        }

        if matches!(
            instruction.opcode,
            OpCode::InitFcall | OpCode::InitMethodCall
        ) {
            let (num_args, parameter_offset, guard) = match instruction.opcode {
                OpCode::InitFcall => (
                    instruction.op1 as usize,
                    0usize,
                    ScalarLongCallGuard::FunctionCache {
                        cache_ip: ip as u32,
                    },
                ),
                OpCode::InitMethodCall if instruction.op1_type == OpType::Cv => {
                    let receiver_cv = instruction.op1 as u32;
                    let receiver_index = receiver_cv.checked_sub(common.sig.this_offset)?;
                    if receiver_index >= public_args
                        || object_argument_mask & (1u8 << receiver_index) == 0
                    {
                        return None;
                    }
                    (
                        instruction.extended_value as usize,
                        1usize,
                        ScalarLongCallGuard::MethodCache {
                            cache_ip: ip as u32,
                            receiver_slot: instruction.op1,
                        },
                    )
                }
                _ => return None,
            };
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
                    || send.op2 as usize != argument_index + parameter_offset
                {
                    return None;
                }
                arguments.push(composed_scalar_long_source(
                    function,
                    long_argument_mask,
                    &temporary_results,
                    send.op1_type,
                    send.op1,
                )?);
            }
            let do_fcall = &op_array.instructions[ip + 1 + num_args];
            if do_fcall.opcode != OpCode::DoFcall
                || do_fcall.known_result_type() == KnownScalarType::String
                || !matches!(do_fcall.result_type, OpType::Tmp | OpType::Var)
            {
                return None;
            }
            let result_index = operations.len() as u8;
            operations.push(ComposedScalarLongOp::Call(ScalarLongCall {
                guard,
                arguments: arguments.into_boxed_slice(),
            }));
            temporary_results.insert(do_fcall.result, ScalarLongSource::Temporary(result_index));
            contains_call = true;
            ip += num_args + 2;
            continue;
        }

        if instruction.opcode == OpCode::AssignCv {
            bind_composed_scalar_long_local(
                function,
                long_argument_mask,
                instruction,
                &mut temporary_results,
            )?;
            ip += 1;
            continue;
        }

        let kind = scalar_long_op_kind(instruction.opcode)?;
        if operations.len() == COMPOSED_SCALAR_LONG_PLAN_MAX_OPS
            || !matches!(instruction.result_type, OpType::Tmp | OpType::Var)
        {
            return None;
        }
        let lhs = composed_scalar_long_source(
            function,
            long_argument_mask,
            &temporary_results,
            instruction.op1_type,
            instruction.op1,
        )?;
        let rhs = composed_scalar_long_source(
            function,
            long_argument_mask,
            &temporary_results,
            instruction.op2_type,
            instruction.op2,
        )?;
        let result_index = operations.len() as u8;
        operations.push(ComposedScalarLongOp::Arithmetic(ScalarLongOp {
            kind,
            lhs,
            rhs,
        }));
        temporary_results.insert(
            instruction.result,
            ScalarLongSource::Temporary(result_index),
        );
        ip += 1;
    }

    None
}

/// Recognize a bounded wrapper around one dynamic closure call. This plan does
/// not assume which closure will occupy the source: the live Value, exact
/// function signature, empty initial capture envelope and scalar leaf plan are
/// all guarded when a typed region is entered.
#[cfg(all(feature = "quick-loops", feature = "jit-prototype"))]
#[cold]
#[inline(never)]
fn build_indirect_scalar_long_function_plan(
    function: &UserFunction,
) -> Option<Box<IndirectScalarLongFunctionPlan>> {
    let common = &function.common;
    let op_array = &function.op_array;
    let public_args = common.sig.public_arity();
    if !common.plan.call.is_compact_user_call()
        || common.sig.returns_reference
        || common.plan.ret != ReturnStrategy::Fast
        || common.sig.is_variadic
        || common.sig.ref_args != 0
        || public_args != common.sig.required_num_args
        || public_args > SCALAR_LONG_PLAN_MAX_ARGS
        || op_array.instructions.len() > 10
        || !matches!(
            common.sig.return_type_hint,
            ParamTypeHint::None | ParamTypeHint::Mixed | ParamTypeHint::Int
        )
    {
        return None;
    }

    let mut ip = 0usize;
    let callable = if let Some(fetch) = op_array.instructions.get(ip)
        && fetch.opcode == OpCode::FetchObjR
        && common.sig.this_offset == 1
        && fetch.op1_type == OpType::Cv
        && fetch.op1 == 0
        && fetch.op2_type == OpType::Const
        && op_array
            .literals
            .get(fetch.op2 as usize)
            .and_then(Value::as_str)
            .is_some()
        && matches!(fetch.result_type, OpType::Tmp | OpType::Var)
    {
        let cache_ip = u16::try_from(ip).ok()?;
        let mut callable_type = fetch.result_type;
        let mut callable_slot = fetch.result;
        ip += 1;
        if let Some(assign) = op_array.instructions.get(ip)
            && assign.opcode == OpCode::AssignCv
            && assign.op1_type == OpType::Cv
            && assign.op2_type == callable_type
            && assign.op2 == callable_slot
        {
            let destination = u32::from(assign.op1);
            let argument_start = common.sig.this_offset;
            let argument_end = argument_start + public_args;
            if destination < argument_start
                || (destination >= argument_start && destination < argument_end)
            {
                return None;
            }
            callable_type = OpType::Cv;
            callable_slot = assign.op1;
            ip += 1;
        }
        let initializer = op_array.instructions.get(ip)?;
        if initializer.opcode != OpCode::InitDynamicCall
            || initializer.op1_type != callable_type
            || initializer.op1 != callable_slot
        {
            return None;
        }
        IndirectScalarLongCallable::ReceiverProperty { cache_ip }
    } else {
        let initializer = op_array.instructions.get(ip)?;
        if initializer.opcode != OpCode::InitDynamicCall || initializer.op1_type != OpType::Cv {
            return None;
        }
        let callable_cv = u32::from(initializer.op1);
        let callable_index = callable_cv.checked_sub(common.sig.this_offset)?;
        if callable_index >= public_args {
            return None;
        }
        IndirectScalarLongCallable::PublicArgument(u8::try_from(callable_index).ok()?)
    };

    let initializer = op_array.instructions.get(ip)?;
    let argument_count = usize::try_from(initializer.extended_value).ok()?;
    if argument_count > SCALAR_LONG_PLAN_MAX_ARGS as usize {
        return None;
    }
    let empty_temporaries = HashMap::new();
    let mut arguments = Vec::with_capacity(argument_count);
    for argument_index in 0..argument_count {
        let send = op_array.instructions.get(ip + 1 + argument_index)?;
        if !matches!(send.opcode, OpCode::SendVal | OpCode::SendVarEx)
            || send.op2 as usize != argument_index
        {
            return None;
        }
        let source = scalar_long_source(
            op_array,
            &empty_temporaries,
            common.sig.this_offset,
            public_args,
            send.op1_type,
            send.op1,
        )?;
        if let ScalarLongSource::Input(index) = source
            && !matches!(
                common
                    .sig
                    .param_type_hints
                    .get(index as usize)
                    .unwrap_or(&ParamTypeHint::None),
                ParamTypeHint::None | ParamTypeHint::Mixed | ParamTypeHint::Int
            )
        {
            return None;
        }
        arguments.push(source);
    }

    let do_ip = ip + 1 + argument_count;
    let do_fcall = op_array.instructions.get(do_ip)?;
    let return_instruction = op_array.instructions.get(do_ip + 1)?;
    if do_fcall.opcode != OpCode::DoFcall
        || !matches!(do_fcall.result_type, OpType::Tmp | OpType::Var)
        || return_instruction.opcode != OpCode::Return
        || return_instruction.extended_value == 0
        || return_instruction.op1_type != do_fcall.result_type
        || return_instruction.op1 != do_fcall.result
        || op_array.instructions[do_ip + 2..]
            .iter()
            .any(|instruction| {
                instruction.opcode != OpCode::Return || instruction.extended_value != 0
            })
    {
        return None;
    }

    Some(Box::new(IndirectScalarLongFunctionPlan {
        public_args: public_args as u8,
        callable,
        arguments: arguments.into_boxed_slice(),
    }))
}

/// Recognize a typed composed body whose borrowed String results are consumed
/// by scalar operations such as `strlen`. Runtime still guards every leaf and
/// falls back before materializing or observing a speculative value.
pub(crate) fn build_composed_typed_long_function_plan(
    function: &UserFunction,
) -> Option<Box<ComposedTypedLongFunctionPlan>> {
    let common = &function.common;
    let op_array = &function.op_array;
    let public_args = common.sig.public_arity();
    let (long_argument_mask, object_argument_mask, string_argument_mask) =
        composed_typed_argument_masks(function)?;
    if common.sig.returns_reference
        || common.plan.ret != ReturnStrategy::Fast
        || op_array.instructions.len() > 32
    {
        return None;
    }

    let mut temporary_results = HashMap::new();
    let mut string_results = HashMap::new();
    for argument in 0..public_args as usize {
        if string_argument_mask & (1u8 << argument) != 0 {
            string_results.insert(
                common.sig.param_cv_index(argument as u32) as u16,
                ScalarStringSource::Input(argument as u8),
            );
        }
    }
    let mut operations = Vec::new();
    let mut contains_string = false;
    let mut ip = 0usize;

    while ip < op_array.instructions.len() {
        let instruction = &op_array.instructions[ip];
        if instruction.opcode == OpCode::ReleaseTemps {
            ip += 1;
            continue;
        }
        if instruction.opcode == OpCode::Return {
            if instruction.extended_value == 0 || !contains_string {
                return None;
            }
            let result = composed_scalar_long_source(
                function,
                long_argument_mask,
                &temporary_results,
                instruction.op1_type,
                instruction.op1,
            )?;
            return Some(Box::new(ComposedTypedLongFunctionPlan {
                public_args: public_args as u8,
                long_argument_mask,
                object_argument_mask,
                string_argument_mask,
                program: ScalarLongProgram {
                    operations: operations.into_boxed_slice(),
                    outputs: [result],
                    output_count: 1,
                },
            }));
        }

        if matches!(
            instruction.opcode,
            OpCode::InitFcall | OpCode::InitMethodCall
        ) {
            let (num_args, parameter_offset, guard) = match instruction.opcode {
                OpCode::InitFcall => (
                    instruction.op1 as usize,
                    0usize,
                    ScalarLongCallGuard::FunctionCache {
                        cache_ip: ip as u32,
                    },
                ),
                OpCode::InitMethodCall if instruction.op1_type == OpType::Cv => {
                    let receiver_cv = instruction.op1 as u32;
                    let receiver_index = receiver_cv.checked_sub(common.sig.this_offset)?;
                    if receiver_index >= public_args
                        || object_argument_mask & (1u8 << receiver_index) == 0
                    {
                        return None;
                    }
                    (
                        instruction.extended_value as usize,
                        1usize,
                        ScalarLongCallGuard::MethodCache {
                            cache_ip: ip as u32,
                            receiver_slot: instruction.op1,
                        },
                    )
                }
                _ => return None,
            };
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
                    || send.op2 as usize != argument_index + parameter_offset
                {
                    return None;
                }
                arguments.push(composed_scalar_long_source(
                    function,
                    long_argument_mask,
                    &temporary_results,
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
            let call = ScalarLongCall {
                guard,
                arguments: arguments.into_boxed_slice(),
            };
            if do_fcall.known_result_type() == KnownScalarType::String {
                operations.push(ComposedTypedLongOp::StringCall(call));
                contains_string = true;
                string_results.insert(do_fcall.result, ScalarStringSource::Temporary(result_index));
            } else {
                operations.push(ComposedTypedLongOp::Call(call));
                temporary_results
                    .insert(do_fcall.result, ScalarLongSource::Temporary(result_index));
            }
            ip += num_args + 2;
            continue;
        }

        if instruction.opcode == OpCode::AssignCv {
            if matches!(instruction.op2_type, OpType::Tmp | OpType::Var | OpType::Cv)
                && string_results.contains_key(&instruction.op2)
            {
                let destination = instruction.op1 as u32;
                let argument_start = common.sig.this_offset;
                let argument_end = argument_start + public_args;
                if instruction.op1_type != OpType::Cv
                    || destination < argument_start
                    || (destination >= argument_start && destination < argument_end)
                {
                    return None;
                }
                let source = *string_results.get(&instruction.op2)?;
                string_results.insert(instruction.op1, source);
                ip += 1;
                continue;
            }
            bind_composed_scalar_long_local(
                function,
                long_argument_mask,
                instruction,
                &mut temporary_results,
            )?;
            ip += 1;
            continue;
        }

        if matches!(
            instruction.opcode,
            OpCode::Concat | OpCode::Concat_StringString
        ) {
            if operations.len() == COMPOSED_SCALAR_LONG_PLAN_MAX_OPS
                || !matches!(instruction.result_type, OpType::Tmp | OpType::Var)
            {
                return None;
            }
            let (source_operand, literal_operand) =
                if matches!(instruction.op1_type, OpType::Tmp | OpType::Var | OpType::Cv)
                    && string_results.contains_key(&instruction.op1)
                    && instruction.op2_type == OpType::Const
                {
                    (instruction.op1, instruction.op2)
                } else if matches!(instruction.op2_type, OpType::Tmp | OpType::Var | OpType::Cv)
                    && string_results.contains_key(&instruction.op2)
                    && instruction.op1_type == OpType::Const
                {
                    (instruction.op2, instruction.op1)
                } else {
                    return None;
                };
            let source = *string_results.get(&source_operand)?;
            let literal_len = u32::try_from(
                op_array
                    .literals
                    .get(literal_operand as usize)?
                    .as_str()?
                    .len(),
            )
            .ok()?;
            let result_index = operations.len() as u8;
            operations.push(ComposedTypedLongOp::StringConcatLiteral {
                value: source,
                literal_len,
            });
            string_results.insert(
                instruction.result,
                ScalarStringSource::Temporary(result_index),
            );
            ip += 1;
            continue;
        }

        if matches!(
            instruction.opcode,
            OpCode::Strlen | OpCode::Strlen_Cv | OpCode::Strlen_String
        ) && matches!(instruction.op1_type, OpType::Tmp | OpType::Var | OpType::Cv)
        {
            let source = *string_results.get(&instruction.op1)?;
            if operations.len() == COMPOSED_SCALAR_LONG_PLAN_MAX_OPS
                || !matches!(instruction.result_type, OpType::Tmp | OpType::Var)
            {
                return None;
            }
            let result_index = operations.len() as u8;
            operations.push(ComposedTypedLongOp::StringLength(source));
            contains_string = true;
            temporary_results.insert(
                instruction.result,
                ScalarLongSource::Temporary(result_index),
            );
            ip += 1;
            continue;
        }

        let kind = scalar_long_op_kind(instruction.opcode)?;
        if operations.len() == COMPOSED_SCALAR_LONG_PLAN_MAX_OPS
            || !matches!(instruction.result_type, OpType::Tmp | OpType::Var)
        {
            return None;
        }
        let lhs = composed_scalar_long_source(
            function,
            long_argument_mask,
            &temporary_results,
            instruction.op1_type,
            instruction.op1,
        )?;
        let rhs = composed_scalar_long_source(
            function,
            long_argument_mask,
            &temporary_results,
            instruction.op2_type,
            instruction.op2,
        )?;
        let result_index = operations.len() as u8;
        operations.push(ComposedTypedLongOp::Arithmetic(ScalarLongOp {
            kind,
            lhs,
            rhs,
        }));
        temporary_results.insert(
            instruction.result,
            ScalarLongSource::Temporary(result_index),
        );
        ip += 1;
    }

    None
}

#[cfg(all(feature = "quick-loops", feature = "jit-prototype"))]
fn captured_typed_input_index(common: &FunctionCommon, capture_count: u8, cv: u16) -> Option<u8> {
    let public_args = common.sig.public_arity();
    if u32::from(cv) >= common.sig.this_offset
        && u32::from(cv) < common.sig.this_offset + public_args
    {
        return u8::try_from(u32::from(cv) - common.sig.this_offset).ok();
    }
    let capture_start = common.sig.parameter_cv_count();
    let capture = u32::from(cv).checked_sub(capture_start)?;
    (capture < u32::from(capture_count)).then(|| u8::try_from(public_args + capture).ok())?
}

#[cfg(all(feature = "quick-loops", feature = "jit-prototype"))]
fn captured_typed_long_source(
    function: &UserFunction,
    capture_count: u8,
    long_input_mask: u8,
    temporary_results: &HashMap<u16, ScalarLongSource>,
    op_type: OpType,
    operand: u16,
) -> Option<ScalarLongSource> {
    match op_type {
        OpType::Cv => {
            if let Some(input) =
                captured_typed_input_index(&function.common, capture_count, operand)
            {
                (long_input_mask & (1u8 << input) != 0)
                    .then_some(ScalarLongSource::Input(u16::from(input)))
            } else {
                temporary_results.get(&operand).copied()
            }
        }
        OpType::Const => function
            .op_array
            .literals
            .get(operand as usize)
            .and_then(Value::as_long)
            .map(ScalarLongSource::Constant),
        OpType::Tmp | OpType::Var => temporary_results.get(&operand).copied(),
        OpType::Unused => None,
    }
}

/// Build a side-effect-free scalar program for a closure whose lexical
/// captures are immutable inputs. This is intentionally constructed only
/// from the closure declaration; live capture values and types are guarded at
/// region entry and then bound to an ordinary scalar leaf plan.
#[cfg(all(feature = "quick-loops", feature = "jit-prototype"))]
pub(super) fn build_captured_typed_long_function_plan(
    function: &UserFunction,
    capture_count: u8,
) -> Option<Box<CapturedTypedLongFunctionPlan>> {
    let common = &function.common;
    let op_array = &function.op_array;
    let public_args = common.sig.public_arity();
    let input_count = public_args.checked_add(u32::from(capture_count))?;
    let capture_start = common.sig.parameter_cv_count();
    let capture_end = capture_start.checked_add(u32::from(capture_count))?;
    if capture_count == 0
        || common.sig.returns_reference
        || input_count > SCALAR_LONG_PLAN_MAX_ARGS
        || common.sig.this_offset != 0
        || common.sig.is_variadic
        || common.sig.ref_args != 0
        || public_args != common.sig.required_num_args
        || common.plan.ret != ReturnStrategy::Fast
        || op_array.is_generator
        || !op_array.global_vars.is_empty()
        || !op_array.static_vars.is_empty()
        || !op_array.try_entries.is_empty()
        || op_array.instructions.len() > SCALAR_LONG_PLAN_MAX_OPS + 6
        || capture_end > op_array.num_cvs
        || function
            .reference_cvs
            .iter()
            .any(|cv| *cv >= capture_start && *cv < capture_end)
        || !matches!(
            common.sig.return_type_hint,
            ParamTypeHint::None | ParamTypeHint::Mixed | ParamTypeHint::Int
        )
        || common.sig.param_type_hints.iter().any(|hint| {
            !matches!(
                hint,
                ParamTypeHint::None | ParamTypeHint::Mixed | ParamTypeHint::Int
            )
        })
    {
        return None;
    }

    let mut string_input_mask = 0u8;
    for instruction in &op_array.instructions {
        if matches!(
            instruction.opcode,
            OpCode::Strlen | OpCode::Strlen_Cv | OpCode::Strlen_String
        ) && instruction.op1_type == OpType::Cv
        {
            let input = captured_typed_input_index(common, capture_count, instruction.op1)?;
            if u32::from(input) < public_args {
                return None;
            }
            string_input_mask |= 1u8 << input;
        }
    }
    let input_mask = if input_count == 8 {
        u8::MAX
    } else {
        (1u8 << input_count) - 1
    };
    let long_input_mask = input_mask & !string_input_mask;
    let mut temporary_results = HashMap::new();
    let mut string_results = HashMap::new();
    for input in public_args as u8..input_count as u8 {
        if string_input_mask & (1u8 << input) != 0 {
            let capture = u32::from(input) - public_args;
            string_results.insert(
                u16::try_from(capture_start + capture).ok()?,
                ScalarStringSource::Input(input),
            );
        }
    }

    let mut operations = Vec::new();
    for (ip, instruction) in op_array.instructions.iter().enumerate() {
        if instruction.opcode == OpCode::Return {
            if instruction.extended_value == 0 {
                return None;
            }
            let output = captured_typed_long_source(
                function,
                capture_count,
                long_input_mask,
                &temporary_results,
                instruction.op1_type,
                instruction.op1,
            )?;
            if op_array.instructions[ip + 1..]
                .iter()
                .any(|trailing| trailing.opcode != OpCode::Return || trailing.extended_value != 0)
            {
                return None;
            }
            return Some(Box::new(CapturedTypedLongFunctionPlan {
                public_args: public_args as u8,
                capture_count,
                long_input_mask,
                string_input_mask,
                program: ScalarLongProgram {
                    operations: operations.into_boxed_slice(),
                    outputs: [output],
                    output_count: 1,
                },
            }));
        }

        if matches!(
            instruction.opcode,
            OpCode::Strlen | OpCode::Strlen_Cv | OpCode::Strlen_String
        ) {
            if operations.len() == SCALAR_LONG_PLAN_MAX_OPS
                || !matches!(instruction.result_type, OpType::Tmp | OpType::Var)
            {
                return None;
            }
            let source = *string_results.get(&instruction.op1)?;
            let result_index = operations.len() as u8;
            operations.push(ComposedTypedLongOp::StringLength(source));
            temporary_results.insert(
                instruction.result,
                ScalarLongSource::Temporary(result_index),
            );
            continue;
        }

        let kind = scalar_long_op_kind(instruction.opcode)?;
        if operations.len() == SCALAR_LONG_PLAN_MAX_OPS
            || !matches!(instruction.result_type, OpType::Tmp | OpType::Var)
        {
            return None;
        }
        let lhs = captured_typed_long_source(
            function,
            capture_count,
            long_input_mask,
            &temporary_results,
            instruction.op1_type,
            instruction.op1,
        )?;
        let rhs = captured_typed_long_source(
            function,
            capture_count,
            long_input_mask,
            &temporary_results,
            instruction.op2_type,
            instruction.op2,
        )?;
        let result_index = operations.len() as u8;
        operations.push(ComposedTypedLongOp::Arithmetic(ScalarLongOp {
            kind,
            lhs,
            rhs,
        }));
        temporary_results.insert(
            instruction.result,
            ScalarLongSource::Temporary(result_index),
        );
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
        || common.sig.returns_reference
        || common.sig.public_arity() != 1
        || common.sig.required_num_args != 1
        || common.sig.is_variadic
        || common.sig.ref_args != 0
        || !has_no_type_hints
        || !matches!(
            common.sig.return_type_hint,
            ParamTypeHint::None | ParamTypeHint::Mixed
        )
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
        OpType::Const => {
            LongRecursiveBase::Constant(op_array.literals.get(base_return.op1 as usize)?.as_long()?)
        }
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
        let target_name = op_array.literals.get(initializer.op2 as usize)?.as_str()?;
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
        let delta = op_array.literals.get(subtract.op2 as usize)?.as_long()?;
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
fn build_property_getter_method_plan(function: &UserFunction) -> Option<PropertyGetterMethodPlan> {
    let common = &function.common;
    let op_array = &function.op_array;
    if !common.supports_scalar_long_plan()
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

fn build_property_init_method_plan(function: &UserFunction) -> Option<Box<PropertyInitMethodPlan>> {
    let common = &function.common;
    let op_array = &function.op_array;
    let public_args = common.sig.public_arity();
    if common.sig.this_offset != 1
        || common.sig.returns_reference
        || !common.plan.call.is_compact_user_call()
        || common.plan.ret != ReturnStrategy::Fast
        || common.sig.is_variadic
        || common.sig.ref_args != 0
        || public_args != common.sig.required_num_args
        || public_args > 8
        || op_array.num_cvs != common.sig.num_args
        || op_array.instructions.len() > public_args as usize + 2
    {
        return None;
    }

    let mut assignments = Vec::new();
    let mut saw_return = false;
    for (ip, instruction) in op_array.instructions.iter().enumerate() {
        match instruction.opcode {
            OpCode::AssignObjProp => {
                if saw_return
                    || property_name(op_array, instruction).is_none()
                    || instruction.result_type != OpType::Cv
                    || instruction.result == 0
                    || instruction.result as u32 > public_args
                    || ip > u16::MAX as usize
                {
                    return None;
                }
                assignments.push(PropertyInitAssignment {
                    cache_ip: ip as u16,
                    argument: (instruction.result - 1) as u8,
                });
            }
            OpCode::Return => {
                if instruction.op1_type != OpType::Const
                    || op_array
                        .literals
                        .get(instruction.op1 as usize)?
                        .value_type()
                        != crate::value::ValueType::Null
                {
                    return None;
                }
                saw_return = true;
            }
            _ => return None,
        }
    }
    if !saw_return {
        return None;
    }

    Some(Box::new(PropertyInitMethodPlan {
        public_args: public_args as u8,
        assignments: assignments.into_boxed_slice(),
    }))
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
    if !common.supports_scalar_long_plan()
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

        // A scalar property plan never materializes the canonical TMP/VAR
        // values covered by this statement-boundary cleanup. The baseline
        // opcode remains authoritative when any property/type guard fails;
        // after a proven Long-only operation it is transparent to the plan.
        if instruction.opcode == OpCode::ReleaseTemps {
            ip += 1;
            continue;
        }

        // $this->p = $this->p +/- scalar
        if instruction.opcode == OpCode::FetchObjR && ip + 2 < instructions.len() {
            let arithmetic = &instructions[ip + 1];
            let assign = &instructions[ip + 2];
            if matches!(
                arithmetic.opcode,
                OpCode::Add
                    | OpCode::Add_TmpTmp
                    | OpCode::Add_CvTmp
                    | OpCode::Sub
                    | OpCode::Sub_TmpTmp
                    | OpCode::Sub_CvConst
            ) && assign.opcode == OpCode::AssignObjProp
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
                    operations.push(
                        if matches!(
                            arithmetic.opcode,
                            OpCode::Sub | OpCode::Sub_TmpTmp | OpCode::Sub_CvConst
                        ) {
                            LongPropertyOp::Sub { property, rhs }
                        } else {
                            LongPropertyOp::Add { property, rhs }
                        },
                    );
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
            if matches!(
                comparison.opcode,
                OpCode::IsSmaller | OpCode::IsSmallerOrEqual
            ) && branch.opcode == OpCode::JmpZ
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
                        (
                            long_plan_source(
                                op_array,
                                comparison.op1_type,
                                comparison.op1,
                                public_args,
                            ),
                            true,
                        )
                    } else if comparison.op1_type == instruction.result_type
                        && comparison.op1 == instruction.result
                    {
                        (
                            long_plan_source(
                                op_array,
                                comparison.op2_type,
                                comparison.op2,
                                public_args,
                            ),
                            false,
                        )
                    } else {
                        (None, false)
                    };
                    let candidate = candidate?;
                    let assigned =
                        long_plan_source(op_array, assign.result_type, assign.result, public_args)?;
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
                        LongPropertyOp::Min {
                            property,
                            candidate,
                        }
                    } else {
                        LongPropertyOp::Max {
                            property,
                            candidate,
                        }
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
                register_long_plan_property(&mut properties, &mut property_indices, name, ip, 1)?;
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
            let property =
                register_long_plan_property(&mut properties, &mut property_indices, name, ip, 3)?;
            operations.push(LongPropertyOp::Set { property, value });
            ip += 1;
            continue;
        }

        if instruction.opcode == OpCode::Return {
            // A discarded call result does not erase the observable `void`
            // contract. Only the compiler's bare/implicit return form may be
            // represented by a frame-free property mutator; `return expr;`
            // must stay canonical so PHP emits its normal TypeError.
            if matches!(common.sig.return_type_hint, ParamTypeHint::Void)
                && instruction.extended_value != 0
            {
                return None;
            }
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
                prefer_ref_args: 0,
                returns_reference: false,
                this_offset: 0,
                param_type_hints: vec![],
                param_names,
                return_type_hint: ParamTypeHint::None,
            },
            frame: FrameLayout {
                num_cvs: num_args,
                num_temps: 0,
                total_slots,
            },
            // Fixed-arity internal functions have no VM-level type hints,
            // references, or variadic packing. DoFcall can therefore run the
            // handler after a compact arity/hole check and leave named or
            // otherwise exceptional calls to the full path.
            plan: CallPlan::without_flags(
                CallStrategy::Fast,
                ReturnStrategy::Full,
                CleanupMode::ScanAll,
            ),
            call_count: Cell::new(0),
            hot_status: Cell::new(HotStatus::Cold),
        },
        handler,
        direct_handler: None,
        raw_variadic_handler: None,
        raw_variadic_all_positional: false,
        handler_validates_types: false,
        exact_arity_diagnostics: false,
        deprecation: None,
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
    let mut function = make_internal_function(handler, num_args, required_num_args, param_names);
    function.direct_handler = Some(direct_handler);
    function
}

/// Finalize a user method after the hidden method slot has been reserved at CV 0.
///
/// `make_user_function_typed()` cannot classify a method as FastScalar while
/// `this_offset` is still zero: `num_args` already includes the hidden `$this`
/// slot while `required_num_args` intentionally counts only public arguments.
/// Re-run that classification once the public signature is known.
pub fn finalize_user_method(
    mut function: UserFunction,
    method_name: &str,
    is_static: bool,
) -> UserFunction {
    function.common.sig.this_offset = 1;
    function.common.plan.set_static_method(is_static);
    function.op_array.specialize_foreach_target_writes(
        function.common.sig.ref_args,
        function.common.sig.this_offset,
        &function.reference_cvs,
    );

    // A non-static method recovers its late-called class directly from the
    // receiver in CV 0. It never needs the sparse static-call sidecar, so
    // restore any compact call/return strategy that the body-only constructor
    // conservatively withheld after seeing a late-static operation.
    if !is_static {
        function.common.plan.set_needs_late_static_scope(false);
        function
            .common
            .plan
            .set_has_embedded_late_static_scope(false);
        for instruction in &mut function.op_array.instructions {
            if matches!(
                instruction.opcode,
                OpCode::FetchLateStaticProp
                    | OpCode::AssignLateStaticProp
                    | OpCode::FetchLateClassConst
                    | OpCode::FetchLateDynamicClassConst
            ) {
                instruction._pad &= !LATE_STATIC_PROP_EMBEDDED_SCOPE;
            }
        }
        if !function.common.sig.returns_reference
            && typed_function_supports_fast_return(
                &function.op_array,
                &function.common.sig.return_type_hint,
            )
        {
            function.common.plan.ret = ReturnStrategy::Fast;
        }
    }

    // `$this` ownership is independent of the public argument ABI. Every
    // synchronous method executes while its caller still owns the receiver,
    // so the callee can borrow CV 0 unless it directly transfers that slot as
    // its return value. Generators are excluded because their frame outlives
    // the initiating call.
    let borrow_this = !is_static
        && !function.op_array.is_generator
        // Borrowed slots are represented by an intentionally-clear ownership
        // bit. Frames wider than the bitmap use full-scan cleanup and must own
        // `$this` conventionally.
        && function.op_array.num_cvs + function.op_array.num_temps <= 64
        && !function.op_array.instructions.iter().any(|instruction| {
            instruction.opcode == OpCode::Return
                && instruction.op1_type == OpType::Cv
                && instruction.op1 == 0
        });
    function.common.plan.set_borrow_this(borrow_this);
    function.borrowable_heap_args = build_borrowable_heap_args(&function);

    let common = &function.common;
    let scalar_strategy = common.sig.declared_scalar_call_strategy();
    let can_use_fast_scalar = !common.plan.has_call_diagnostic_attribute()
        && !common.sig.returns_reference
        && scalar_strategy.is_some()
        && !common.plan.needs_late_static_scope()
        && !common.sig.is_variadic
        && common.sig.ref_args == 0
        && common.sig.public_arity() == common.sig.required_num_args
        && function.op_array.global_vars.is_empty()
        && function.op_array.static_vars.is_empty()
        && function.op_array.try_entries.is_empty()
        && !function.op_array.is_generator
        && !function.op_array.may_access_globals;

    if can_use_fast_scalar {
        function.common.plan.call = scalar_strategy.unwrap();
    }

    if !function.common.plan.has_call_diagnostic_attribute() {
        function.long_property_plan = build_long_property_method_plan(&function);
        function.property_getter_plan = build_property_getter_method_plan(&function);
        function.property_init_plan = build_property_init_method_plan(&function);
        function.binary_long_recursion_plan =
            build_binary_long_recursion_plan(&function, method_name);
        function.scalar_long_plan = build_scalar_long_function_plan(&function);
        function.scalar_double_plan = build_scalar_double_function_plan(&function);
        function.composed_scalar_double_plan =
            build_composed_scalar_double_function_plan(&function);
        function.object_long_plan = build_object_long_function_plan(&function);
        function.object_array_plan = build_object_array_function_plan(&function);
        function.scalar_string_plan = build_scalar_string_function_plan(&function);
        function.composed_scalar_long_plan = build_composed_scalar_long_function_plan(&function);
        function.composed_typed_long_plan = build_composed_typed_long_function_plan(&function);
    }
    #[cfg(all(feature = "quick-loops", feature = "jit-prototype"))]
    {
        let indirect_scalar_long_plan = (!function.common.plan.has_call_diagnostic_attribute())
            .then(|| build_indirect_scalar_long_function_plan(&function))
            .flatten();
        function.set_indirect_scalar_long_plan(indirect_scalar_long_plan);
    }

    function
}

/// Clone a trait method for one concrete composed method name. Ordinary trait
/// methods share their original pointer; this cold path covers independent
/// function-static storage, consumer-specific declaration diagnostics and
/// final-class binding of trait `__CLASS__` sites.
pub fn clone_trait_method_with_static_storage(
    source: &UserFunction,
    class_name: &str,
    method_name: &str,
    is_static: bool,
) -> UserFunction {
    let source_op = &source.op_array;
    let method_name = source_op
        .name
        .rsplit_once("::")
        .map(|(_, source_method)| source_method)
        .filter(|source_method| source_method.eq_ignore_ascii_case(method_name))
        .unwrap_or(method_name);
    let mut op_array = OpArray {
        num_cvs: source_op.num_cvs,
        num_temps: source_op.num_temps,
        trait_class_scope_tmp: source_op.trait_class_scope_tmp,
        instructions: source_op.instructions.clone(),
        source_lines: source_op.source_lines.clone(),
        literals: source_op.literals.clone(),
        try_entries: source_op.try_entries.clone(),
        strict_types: source_op.strict_types,
        is_generator: source_op.is_generator,
        global_vars: source_op.global_vars.clone(),
        static_vars: source_op.static_vars.clone(),
        name: format!("{class_name}::{method_name}"),
        source_file: source_op.source_file.clone(),
        main_scope_vars: source_op.main_scope_vars.clone(),
        all_cvs: source_op.all_cvs.clone(),
        cache: (0..source_op.instructions.len())
            .map(|_| InlineCache::empty())
            .collect(),
        may_access_globals: source_op.may_access_globals,
        block_info: Vec::new(),
        block_counters: Vec::new(),
        block_plans: Vec::new(),
        ip_to_block: Vec::new(),
    };
    let storage_name = op_array.name.clone();
    for instruction in &op_array.instructions {
        if matches!(instruction.opcode, OpCode::CheckStatic | OpCode::BindStatic) {
            op_array.literals[instruction.extended_value as usize] =
                Value::string(storage_name.clone());
        }
    }
    op_array.compute_blocks();
    let signature = &source.common.sig;
    let mut plan = CallPlan::without_flags(
        source.common.plan.call,
        source.common.plan.ret,
        source.common.plan.cleanup,
    );
    plan.set_borrow_this(source.common.plan.borrow_this());
    plan.set_needs_late_static_scope(source.common.plan.needs_late_static_scope());
    plan.set_has_embedded_late_static_scope(source.common.plan.has_embedded_late_static_scope());
    plan.set_needs_trait_class_scope(source.common.plan.needs_trait_class_scope());
    plan.set_has_deprecated_attribute(source.common.plan.has_deprecated_attribute());
    plan.set_has_no_discard_attribute(source.common.plan.has_no_discard_attribute());
    plan.set_has_reference_foreach(source.common.plan.has_reference_foreach());
    let function = UserFunction {
        common: FunctionCommon {
            fn_type: FunctionType::User,
            sig: SignatureInfo {
                num_args: signature.num_args,
                required_num_args: signature.required_num_args,
                is_variadic: signature.is_variadic,
                variadic_cv_index: signature.variadic_cv_index,
                ref_args: signature.ref_args,
                prefer_ref_args: signature.prefer_ref_args,
                returns_reference: signature.returns_reference,
                this_offset: signature.this_offset,
                param_type_hints: signature.param_type_hints.clone(),
                param_names: signature.param_names.clone(),
                return_type_hint: signature.return_type_hint.clone(),
            },
            frame: FrameLayout {
                num_cvs: source.common.frame.num_cvs,
                num_temps: source.common.frame.num_temps,
                total_slots: source.common.frame.total_slots,
            },
            plan,
            call_count: Cell::new(0),
            hot_status: Cell::new(HotStatus::Cold),
        },
        op_array,
        attributes: source.attributes.clone(),
        parameter_attributes: source.parameter_attributes.clone(),
        parameter_default_diagnostics: source.parameter_default_diagnostics.clone(),
        reference_cvs: source.reference_cvs.clone(),
        long_property_plan: None,
        property_getter_plan: None,
        property_init_plan: None,
        binary_long_recursion_plan: None,
        scalar_long_plan: None,
        scalar_double_plan: None,
        composed_scalar_double_plan: None,
        object_long_plan: None,
        object_array_plan: None,
        scalar_string_plan: None,
        composed_scalar_long_plan: None,
        composed_typed_long_plan: None,
        compact_class_guard: Cell::new(0),
        borrowable_heap_args: 0,
        trait_class_scope_cache: source
            .common
            .plan
            .needs_trait_class_scope()
            .then(|| Box::new(crate::vm::function::TraitClassScopeCache::empty())),
    };
    finalize_user_method(function, method_name, is_static)
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
                prefer_ref_args: 0,
                returns_reference: false,
                this_offset: 1,
                param_type_hints: vec![],
                param_names,
                return_type_hint: ParamTypeHint::None,
            },
            frame: FrameLayout {
                num_cvs: num_args,
                num_temps: 0,
                total_slots,
            },
            plan: CallPlan::without_flags(
                CallStrategy::Fast,
                ReturnStrategy::Full,
                CleanupMode::ScanAll,
            ),
            call_count: Cell::new(0),
            hot_status: Cell::new(HotStatus::Cold),
        },
        handler,
        direct_handler: None,
        raw_variadic_handler: None,
        raw_variadic_all_positional: false,
        handler_validates_types: false,
        exact_arity_diagnostics: false,
        deprecation: None,
    }
}

/// Create a variadic InternalFunction for an instance method.
/// `required_num_args` counts explicit arguments; CV 0 remains the hidden
/// receiver and the variadic bucket follows the fixed public arguments.
pub fn make_internal_method_variadic(
    handler: InternalFunctionHandler,
    required_num_args: u32,
    param_names: Vec<String>,
) -> InternalFunction {
    let num_args = required_num_args + 1;
    let variadic_cv_index = num_args;
    let num_cvs = variadic_cv_index + 1;
    let total_slots = crate::vm::frame::CALL_FRAME_SLOTS as u32 + num_cvs;
    InternalFunction {
        common: FunctionCommon {
            fn_type: FunctionType::Internal,
            sig: SignatureInfo {
                num_args,
                required_num_args,
                is_variadic: true,
                variadic_cv_index,
                ref_args: 0,
                prefer_ref_args: 0,
                returns_reference: false,
                this_offset: 1,
                param_type_hints: vec![],
                param_names,
                return_type_hint: ParamTypeHint::None,
            },
            frame: FrameLayout {
                num_cvs,
                num_temps: 0,
                total_slots,
            },
            plan: CallPlan::without_flags(
                CallStrategy::Full,
                ReturnStrategy::Full,
                CleanupMode::ScanAll,
            ),
            call_count: Cell::new(0),
            hot_status: Cell::new(HotStatus::Cold),
        },
        handler,
        direct_handler: None,
        raw_variadic_handler: None,
        raw_variadic_all_positional: false,
        handler_validates_types: false,
        exact_arity_diagnostics: false,
        deprecation: None,
    }
}

/// Create a variadic internal instance method with a raw positional handler.
/// Calls with at most one variadic value can consume their original flat call
/// slots, while wider or named calls retain the canonical packed-array ABI.
pub fn make_internal_method_variadic_raw(
    handler: InternalFunctionHandler,
    raw_variadic_handler: RawVariadicInternalFunctionHandler,
    required_num_args: u32,
    param_names: Vec<String>,
) -> InternalFunction {
    let mut function = make_internal_method_variadic(handler, required_num_args, param_names);
    function.raw_variadic_handler = Some(raw_variadic_handler);
    function.common.plan.call = CallStrategy::Fast;
    function
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
                prefer_ref_args: 0,
                returns_reference: false,
                this_offset: 0,
                param_type_hints: vec![],
                param_names,
                return_type_hint: ParamTypeHint::None,
            },
            frame: FrameLayout {
                num_cvs: num_args,
                num_temps: 0,
                total_slots,
            },
            plan: CallPlan::without_flags(
                CallStrategy::Full,
                ReturnStrategy::Full,
                CleanupMode::ScanAll,
            ),
            call_count: Cell::new(0),
            hot_status: Cell::new(HotStatus::Cold),
        },
        handler,
        direct_handler: None,
        raw_variadic_handler: None,
        raw_variadic_all_positional: false,
        handler_validates_types: false,
        exact_arity_diagnostics: false,
        deprecation: None,
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
                prefer_ref_args: 0,
                returns_reference: false,
                this_offset: 0,
                param_type_hints: vec![],
                param_names,
                return_type_hint: ParamTypeHint::None,
            },
            frame: FrameLayout {
                num_cvs,
                num_temps: 0,
                total_slots,
            },
            plan: CallPlan::without_flags(
                CallStrategy::Full,
                ReturnStrategy::Full,
                CleanupMode::ScanAll,
            ),
            call_count: Cell::new(0),
            hot_status: Cell::new(HotStatus::Cold),
        },
        handler,
        direct_handler: None,
        raw_variadic_handler: None,
        raw_variadic_all_positional: false,
        handler_validates_types: false,
        exact_arity_diagnostics: false,
        deprecation: None,
    }
}

/// Create a by-value variadic InternalFunction with a raw positional handler.
/// Calls with at most one variadic value can use the validated fast-call path;
/// wider and named calls retain the canonical packed-array ABI.
pub fn make_internal_function_variadic_raw(
    handler: InternalFunctionHandler,
    raw_variadic_handler: RawVariadicInternalFunctionHandler,
    required_num_args: u32,
    param_names: Vec<String>,
) -> InternalFunction {
    let mut function = make_internal_function_variadic(handler, required_num_args, param_names);
    function.raw_variadic_handler = Some(raw_variadic_handler);
    function.common.plan.call = CallStrategy::Fast;
    function
}

/// Create a variadic InternalFunction with a by-reference mask for its fixed
/// parameters. Variadic values remain ordinary by-value arguments unless the
/// mask explicitly reaches the variadic public position.
pub fn make_internal_function_variadic_ref(
    handler: InternalFunctionHandler,
    raw_variadic_handler: RawVariadicInternalFunctionHandler,
    required_num_args: u32,
    ref_args: u64,
    param_names: Vec<String>,
) -> InternalFunction {
    let mut function = make_internal_function_variadic(handler, required_num_args, param_names);
    function.common.sig.ref_args = ref_args;
    function.raw_variadic_handler = Some(raw_variadic_handler);
    function
}

/// Create a variadic by-reference internal whose positional handler consumes
/// the original flat argument slots for every arity. The handler must snapshot
/// all readable arguments before its first mutation; named arguments retain
/// the canonical packed ABI and use `handler`.
pub fn make_internal_function_variadic_ref_raw_all(
    handler: InternalFunctionHandler,
    raw_variadic_handler: RawVariadicInternalFunctionHandler,
    required_num_args: u32,
    ref_args: u64,
    param_names: Vec<String>,
) -> InternalFunction {
    let mut function = make_internal_function_variadic_ref(
        handler,
        raw_variadic_handler,
        required_num_args,
        ref_args,
        param_names,
    );
    function.raw_variadic_all_positional = true;
    function
}

/// Create a variadic internal function whose fixed and variadic parameters
/// use PHP's legacy prefer-reference calling convention.
pub fn make_internal_function_variadic_prefer_ref(
    handler: InternalFunctionHandler,
    required_num_args: u32,
    param_names: Vec<String>,
) -> InternalFunction {
    let num_cvs = required_num_args + 1;
    let variadic_cv_index = required_num_args;
    let total_slots = crate::vm::frame::CALL_FRAME_SLOTS as u32 + num_cvs;
    let reference_args = if variadic_cv_index < 63 {
        (1u64 << (variadic_cv_index + 1)) - 1
    } else {
        u64::MAX
    };
    InternalFunction {
        common: FunctionCommon {
            fn_type: FunctionType::Internal,
            sig: SignatureInfo {
                num_args: required_num_args,
                required_num_args,
                is_variadic: true,
                variadic_cv_index,
                ref_args: reference_args,
                prefer_ref_args: reference_args,
                returns_reference: false,
                this_offset: 0,
                param_type_hints: vec![],
                param_names,
                return_type_hint: ParamTypeHint::None,
            },
            frame: FrameLayout {
                num_cvs,
                num_temps: 0,
                total_slots,
            },
            plan: CallPlan::without_flags(
                CallStrategy::Full,
                ReturnStrategy::Full,
                CleanupMode::ScanAll,
            ),
            call_count: Cell::new(0),
            hot_status: Cell::new(HotStatus::Cold),
        },
        handler,
        direct_handler: None,
        raw_variadic_handler: None,
        raw_variadic_all_positional: false,
        handler_validates_types: false,
        exact_arity_diagnostics: false,
        deprecation: None,
    }
}
