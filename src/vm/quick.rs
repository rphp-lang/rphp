//! Quickened, guarded execution regions.
//!
//! This module deliberately starts with one family of closed scalar loops. The
//! baseline bytecode remains the semantic source of truth; detection only
//! creates a compact description that `execute_ex` may run after the backedge
//! becomes hot.

use crate::compiler::OpArray;
use crate::value::Value;
use crate::vm::function::{
    ScalarLongCallGuard, ScalarLongFunctionPlan, ScalarLongOp, ScalarLongOpKind,
    ScalarLongProgram, ScalarLongSource,
};
use crate::vm::instruction::OpType;
use crate::vm::opcode::OpCode;

/// Number of executions of the same backward edge before quickening it.
pub const QUICK_LOOP_HOT_THRESHOLD: u32 = 32;
/// Consecutive failed activations before a region stays in baseline.
pub const QUICK_LOOP_FAILURE_LIMIT: u32 = 3;
/// Counter states reserve one full hotness interval per failed activation.
pub const QUICK_LOOP_COUNTER_STRIDE: u32 = QUICK_LOOP_HOT_THRESHOLD + 1;
pub const QUICK_LOOP_DISABLED: u32 = u32::MAX;
/// Maximum allocation-free inline-cache width for changing string array keys.
pub(super) const QUICK_STRING_FETCH_CACHE_LIMIT: usize = 4;

#[derive(Debug, Clone, Copy)]
pub enum QuickLongBound {
    Cv(u16),
    Const(i64),
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum QuickIncrementKind {
    Pre,
    Post,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum QuickArrayIndex {
    Long(QuickLongOperand),
    StringLiteral(u16),
    /// Loop-invariant CV whose runtime value is normalized as an array key.
    ValueSlot(u16),
}

#[derive(Debug, Clone)]
pub enum QuickLongTerm {
    /// Add the induction variable directly to the accumulator.
    Induction,
    /// First compute induction + constant into a TMP, then accumulate it.
    InductionPlusConst {
        addend: i64,
        term_tmp: u16,
        term_ip: usize,
    },
    /// First compute induction + an invariant CV into a TMP, then accumulate it.
    InductionPlusCv {
        addend_cv: u16,
        term_tmp: u16,
        term_ip: usize,
    },
    /// Read a long from an invariant array using an integer or string key.
    ArrayIndex {
        array_cv: u16,
        index: QuickArrayIndex,
        term_tmp: u16,
        destination: Option<u16>,
        fetch_ip: usize,
    },
    /// Byte length of an invariant string CV, produced by Strlen_Cv.
    StringLength {
        string_cv: u16,
        term_tmp: u16,
        term_ip: usize,
    },
    /// Absolute value of a long CV, produced by DirectInternalCall1(abs).
    AbsLong {
        operand_cv: u16,
        term_tmp: u16,
        term_ip: usize,
    },
    /// Result of a direct user function call whose cached target is guarded at
    /// runtime as a compiler-proven pure scalar function. Arguments remain
    /// ordinary PHP operands in the baseline program; the quick runner reads
    /// their retained long values without constructing a call frame.
    ScalarFunctionCall {
        guard: ScalarLongCallGuard,
        do_fcall_ip: usize,
        long_input_mask: u64,
        argument_plan: Box<ScalarLongProgram>,
        argument_count: u8,
        term_tmp: u16,
    },
    /// A scalar method call tree whose object receivers are invariant CVs.
    /// Runtime validates each receiver class against the monomorphic method
    /// cache before executing any compiler-proven scalar body.
    ScalarMethodCall {
        guard: ScalarLongCallGuard,
        do_fcall_ip: usize,
        long_input_mask: u64,
        object_input_mask: u64,
        argument_count: u8,
        term_tmp: u16,
    },
}

/// Region for the compiler shapes produced by:
///
/// ```php
/// for (...; $i < $limit; $i++) { // `++$i` is supported as well
///     $accumulator += $i;
///     // or: $accumulator += $i + INTEGER_CONSTANT;
///     // or: $accumulator += $i + $loop_invariant_cv;
///     // or: $accumulator += $packed_array[$i];
///     // or: $value = $array['key']; $accumulator += $value;
///     // or: $accumulator += strlen($loop_invariant_string);
///     // or: $accumulator += abs($long_cv);
///     // or: $accumulator += pureScalar($long_cv, INTEGER_CONSTANT);
/// }
/// ```
///
/// The baseline region contains comparison, conditional exit, accumulation,
/// assignment, increment and backward jump, with an optional arithmetic
/// term instruction. All observable state is scalar and every deoptimization
/// point has a precise baseline instruction.
#[derive(Debug, Clone)]
pub struct QuickLongAccumulateLoop {
    pub header_ip: usize,
    pub exit_ip: usize,
    pub induction_cv: u16,
    pub bound: QuickLongBound,
    pub accumulator_cv: u16,
    pub condition_tmp: Option<u16>,
    pub term: QuickLongTerm,
    pub sum_tmp: u16,
    pub increment_kind: QuickIncrementKind,
    pub increment_tmp: Option<u16>,
    pub sum_ip: usize,
    pub increment_ip: usize,
}

/// Guarded induction-only loop:
///
/// ```php
/// for ($i = $start; $i < $bound; $i++) {
/// }
/// ```
///
/// The same region also covers `while` and prefix increment syntax.
#[derive(Debug, Clone, Copy)]
pub struct QuickLongInductionLoop {
    pub header_ip: usize,
    pub exit_ip: usize,
    pub induction_cv: u16,
    pub bound: QuickLongBound,
    pub condition_tmp: Option<u16>,
    pub increment_kind: QuickIncrementKind,
    pub increment_tmp: Option<u16>,
    pub increment_ip: usize,
}

/// Guarded value-only foreach recurrence:
///
/// ```php
/// foreach ($array as $value) {
///     $accumulator += $value;
/// }
/// ```
///
/// The array copy and iterator position have already been initialized by
/// `ForeachInit` when the backedge becomes hot. The runner owns only the
/// closed `ForeachNext` through backward-jump region.
#[derive(Debug, Clone, Copy)]
pub struct QuickForeachLongAccumulateLoop {
    pub header_ip: usize,
    pub exit_ip: usize,
    pub array_tmp: u16,
    pub position_tmp: u16,
    pub value_cv: u16,
    pub done_tmp: u16,
    pub accumulator_cv: u16,
    pub sum_tmp: u16,
    pub sum_ip: usize,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum QuickLongOperand {
    Slot(u16),
    Const(i64),
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum QuickLongCondition {
    Lt { lhs: u16, rhs: QuickLongOperand },
    Eq { lhs: u16, rhs: QuickLongOperand },
}

const QUICK_LONG_TARGET_OP: u32 = 1 << 31;
const QUICK_LONG_TARGET_EXIT: u32 = 1 << 30;
const QUICK_LONG_TARGET_KIND_MASK: u32 = QUICK_LONG_TARGET_OP | QUICK_LONG_TARGET_EXIT;
const QUICK_LONG_TARGET_PAYLOAD_MASK: u32 = !QUICK_LONG_TARGET_KIND_MASK;

/// A compact control-flow target for a typed scalar loop.
///
/// Targets are initially baseline instruction positions. Once the complete
/// region is known, internal targets are rewritten to typed-operation indices
/// and forward exits retain their baseline instruction position.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct QuickLongTarget(u32);

impl QuickLongTarget {
    #[inline]
    fn unresolved(ip: usize) -> Option<Self> {
        (ip <= QUICK_LONG_TARGET_PAYLOAD_MASK as usize).then_some(Self(ip as u32))
    }

    #[inline]
    fn unresolved_ip(self) -> Option<usize> {
        (self.0 & QUICK_LONG_TARGET_KIND_MASK == 0).then_some(self.0 as usize)
    }

    fn resolve(
        &mut self,
        header_ip: usize,
        backedge_ip: usize,
        instruction_len: usize,
        ip_to_op: &[u16],
    ) -> Option<()> {
        let ip = self.unresolved_ip()?;
        if (header_ip..=backedge_ip).contains(&ip) {
            let op_index = *ip_to_op.get(ip - header_ip)?;
            if op_index == u16::MAX {
                return None;
            }
            self.0 = QUICK_LONG_TARGET_OP | u32::from(op_index);
        } else {
            if ip <= backedge_ip || ip >= instruction_len {
                return None;
            }
            self.0 = QUICK_LONG_TARGET_EXIT | ip as u32;
        }
        Some(())
    }

    #[inline(always)]
    pub fn op_index(self) -> Option<usize> {
        (self.0 & QUICK_LONG_TARGET_KIND_MASK == QUICK_LONG_TARGET_OP)
            .then_some((self.0 & QUICK_LONG_TARGET_PAYLOAD_MASK) as usize)
    }

    #[inline(always)]
    pub fn exit_ip(self) -> Option<usize> {
        (self.0 & QUICK_LONG_TARGET_KIND_MASK == QUICK_LONG_TARGET_EXIT)
            .then_some((self.0 & QUICK_LONG_TARGET_PAYLOAD_MASK) as usize)
    }
}

/// A typed operation in a guarded scalar loop region.
///
/// Operations retain baseline instruction positions so arithmetic overflow can
/// resume without replaying already committed operations.
#[derive(Debug, Clone, Copy)]
pub enum QuickLongOp {
    BranchUnlessLt {
        lhs: u16,
        rhs: QuickLongOperand,
        condition_tmp: Option<u16>,
        false_target: QuickLongTarget,
        next_target: QuickLongTarget,
        resume_ip: usize,
    },
    BranchUnlessEq {
        lhs: u16,
        rhs: QuickLongOperand,
        condition_tmp: Option<u16>,
        false_target: QuickLongTarget,
        next_target: QuickLongTarget,
        resume_ip: usize,
    },
    ModConst {
        value: u16,
        divisor: i64,
        result: u16,
        next_target: QuickLongTarget,
        resume_ip: usize,
    },
    FetchArrayLong {
        array: u16,
        index: QuickArrayIndex,
        result: u16,
        destination: Option<u16>,
        next_target: QuickLongTarget,
        resume_ip: usize,
    },
    Add {
        lhs: u16,
        rhs: u16,
        result: u16,
        next_target: QuickLongTarget,
        resume_ip: usize,
    },
    AddAssign {
        lhs: u16,
        rhs: u16,
        result: u16,
        destination: u16,
        next_target: QuickLongTarget,
        add_resume_ip: usize,
    },
    ConditionalAddAssign {
        condition: QuickLongCondition,
        condition_tmp: Option<u16>,
        lhs: u16,
        rhs: u16,
        result: u16,
        destination: u16,
        next_target: QuickLongTarget,
        condition_resume_ip: usize,
        add_resume_ip: usize,
    },
    AddAddAssign {
        first_lhs: u16,
        first_rhs: u16,
        first_result: u16,
        second_lhs: u16,
        second_rhs: u16,
        second_result: u16,
        destination: u16,
        next_target: QuickLongTarget,
        first_resume_ip: usize,
        second_resume_ip: usize,
    },
    /// Frame-free compiler-proven property mutator with zero to eight Long
    /// arguments. Dispatch and property layout are guarded at region entry.
    PropertyMethodCall {
        guard: ScalarLongCallGuard,
        arguments: [QuickLongOperand; 8],
        argument_count: u8,
        next_target: QuickLongTarget,
        resume_ip: usize,
    },
    /// Compiler-proven declared-property getter whose scalar result remains in
    /// the typed loop slot file.
    PropertyGetterCall {
        guard: ScalarLongCallGuard,
        result: u16,
        next_target: QuickLongTarget,
        resume_ip: usize,
    },
    /// Exact PHP evaluation order for `propertyMutator(propertyGetter())`.
    ComposedPropertyCall {
        outer_guard: ScalarLongCallGuard,
        inner_guard: ScalarLongCallGuard,
        next_target: QuickLongTarget,
        resume_ip: usize,
    },
    Assign {
        destination: u16,
        source: u16,
        next_target: QuickLongTarget,
    },
    /// Assign a string literal to a CV retained as a dynamic array key.
    AssignStringLiteral {
        destination: u16,
        literal: u16,
        next_target: QuickLongTarget,
    },
    /// Redirect retained string state to another guarded string CV.
    AssignStringSlot {
        destination: u16,
        source: u16,
        next_target: QuickLongTarget,
    },
    PostInc {
        value: u16,
        result: Option<u16>,
        next_target: QuickLongTarget,
        resume_ip: usize,
    },
    PostIncJump {
        value: u16,
        result: Option<u16>,
        target: QuickLongTarget,
        resume_ip: usize,
    },
    PostIncLoopLt {
        value: u16,
        result: Option<u16>,
        condition_lhs: u16,
        condition_rhs: QuickLongOperand,
        condition_tmp: Option<u16>,
        body_target: QuickLongTarget,
        exit_target: QuickLongTarget,
        resume_ip: usize,
    },
    Jump {
        target: QuickLongTarget,
    },
}

impl QuickLongOp {
    fn resolve_targets(
        &mut self,
        header_ip: usize,
        backedge_ip: usize,
        instruction_len: usize,
        ip_to_op: &[u16],
    ) -> Option<()> {
        let resolve = |target: &mut QuickLongTarget| {
            target.resolve(header_ip, backedge_ip, instruction_len, ip_to_op)
        };
        match self {
            Self::BranchUnlessLt {
                false_target,
                next_target,
                ..
            }
            | Self::BranchUnlessEq {
                false_target,
                next_target,
                ..
            } => {
                resolve(false_target)?;
                resolve(next_target)
            }
            Self::ModConst { next_target, .. }
            | Self::FetchArrayLong { next_target, .. }
            | Self::Add { next_target, .. }
            | Self::AddAssign { next_target, .. }
            | Self::ConditionalAddAssign { next_target, .. }
            | Self::AddAddAssign { next_target, .. }
            | Self::PropertyMethodCall { next_target, .. }
            | Self::PropertyGetterCall { next_target, .. }
            | Self::ComposedPropertyCall { next_target, .. }
            | Self::Assign { next_target, .. }
            | Self::AssignStringLiteral { next_target, .. }
            | Self::AssignStringSlot { next_target, .. }
            | Self::PostInc { next_target, .. } => resolve(next_target),
            Self::PostIncJump { target, .. } | Self::Jump { target } => resolve(target),
            Self::PostIncLoopLt {
                body_target,
                exit_target,
                ..
            } => {
                resolve(body_target)?;
                resolve(exit_target)
            }
        }
    }
}

/// A prevalidated typed program for a closed scalar loop. Arithmetic state is
/// long/bool; selected string CVs may be retained solely as dynamic array keys.
#[derive(Debug, Clone)]
pub struct QuickLongOpsLoop {
    pub header_ip: usize,
    pub backedge_ip: usize,
    pub ops: Vec<QuickLongOp>,
    pub entry_op: u16,
    op_ips: Vec<u32>,
    pub long_input_mask: u64,
    pub long_output_mask: u64,
    pub bool_output_mask: u64,
    pub array_input_mask: u64,
    pub string_input_mask: u64,
    pub string_output_mask: u64,
    pub object_input_mask: u64,
    pub string_cache_capacity: u8,
    pub involved_mask: u64,
}

impl QuickLongOpsLoop {
    #[inline(always)]
    pub fn target_ip(&self, target: QuickLongTarget) -> Option<usize> {
        match target.op_index() {
            Some(index) => self.op_ips.get(index).copied().map(|ip| ip as usize),
            None => target.exit_ip(),
        }
    }
}

fn long_literal(op_array: &OpArray, index: u16) -> Option<i64> {
    op_array
        .literals
        .get(index as usize)
        .and_then(Value::as_long)
}

fn quick_scalar_long_source(
    op_array: &OpArray,
    op_type: OpType,
    operand: u16,
    produced_temporary_slots: &[u16; 8],
    produced_temporary_count: usize,
) -> Option<ScalarLongSource> {
    match op_type {
        OpType::Cv => Some(ScalarLongSource::Input(operand)),
        OpType::Const => long_literal(op_array, operand).map(ScalarLongSource::Constant),
        OpType::Tmp | OpType::Var => produced_temporary_slots[..produced_temporary_count]
            .iter()
            .position(|slot| *slot == operand)
            .and_then(|index| u8::try_from(index).ok())
            .map(ScalarLongSource::Temporary),
        OpType::Unused => None,
    }
}

/// Inline a proven scalar leaf body into the quick call-site argument program.
/// Body inputs become the argument program's outputs and body temporaries are
/// shifted after argument-expression temporaries. The result is still the same
/// context-independent scalar IR, now executable as one guarded region.
pub(crate) fn compose_quick_scalar_leaf_program(
    arguments: &ScalarLongProgram,
    body: &ScalarLongFunctionPlan,
) -> Option<ScalarLongProgram<ScalarLongOp, 1>> {
    const MAX_FUSED_SCALAR_OPS: usize = 16;

    if arguments.output_count != body.public_args
        || arguments.operations.len() > 8
        || body.program.operations.len() > 8
        || body.program.output_count != 1
        || arguments.operations.len() + body.program.operations.len()
            > MAX_FUSED_SCALAR_OPS
    {
        return None;
    }
    let argument_operation_count = arguments.operations.len();
    let remap_body_source = |source| match source {
        ScalarLongSource::Input(index) => arguments
            .outputs
            .get(index as usize)
            .copied()
            .filter(|_| index < u16::from(arguments.output_count)),
        ScalarLongSource::Constant(value) => Some(ScalarLongSource::Constant(value)),
        ScalarLongSource::Temporary(index) => {
            let index = argument_operation_count.checked_add(index as usize)?;
            u8::try_from(index).ok().map(ScalarLongSource::Temporary)
        }
    };

    let mut operations = Vec::with_capacity(
        argument_operation_count + body.program.operations.len(),
    );
    operations.extend(arguments.operations.iter().copied());
    for operation in body.program.operations.iter().copied() {
        operations.push(ScalarLongOp {
            kind: operation.kind,
            lhs: remap_body_source(operation.lhs)?,
            rhs: remap_body_source(operation.rhs)?,
        });
    }
    let result = remap_body_source(body.program.outputs[0])?;
    Some(ScalarLongProgram {
        operations: operations.into_boxed_slice(),
        outputs: [result],
        output_count: 1,
    })
}

fn array_literal_index(op_array: &OpArray, index: u16) -> Option<QuickArrayIndex> {
    let value = op_array.literals.get(index as usize)?;
    if let Some(value) = value.as_long() {
        return Some(QuickArrayIndex::Long(QuickLongOperand::Const(value)));
    }

    let value = value.as_str()?;
    if let Ok(integer) = value.parse::<i64>() {
        if integer.to_string() == value {
            return Some(QuickArrayIndex::Long(QuickLongOperand::Const(integer)));
        }
    }
    Some(QuickArrayIndex::StringLiteral(index))
}

/// Recognize a value-only foreach loop whose complete body adds each long
/// value to one long accumulator.
pub fn detect_foreach_long_accumulate_loop(
    op_array: &OpArray,
    header_ip: usize,
    backedge_ip: usize,
) -> Option<QuickForeachLongAccumulateLoop> {
    if header_ip.checked_add(4)? != backedge_ip || backedge_ip >= op_array.instructions.len() {
        return None;
    }

    let next = op_array.instructions[header_ip];
    let branch = op_array.instructions[header_ip + 1];
    let sum = op_array.instructions[header_ip + 2];
    let assign = op_array.instructions[header_ip + 3];
    let backedge = op_array.instructions[backedge_ip];

    if next.opcode != OpCode::ForeachNext
        || next.op1_type != OpType::Tmp
        || next.op2_type != OpType::Tmp
        || next.result_type != OpType::Tmp
        || next.extended_value >> 16 != 0
        || branch.opcode != OpCode::JmpZ
        || branch.op1_type != OpType::Tmp
        || branch.op1 != next.result
        || branch.op2_type != OpType::Unused
        || sum.opcode != OpCode::Add
        || sum.op1_type != OpType::Cv
        || sum.op2_type != OpType::Cv
        || sum.result_type != OpType::Tmp
        || assign.opcode != OpCode::AssignCv
        || assign.op1_type != OpType::Cv
        || assign.op2_type != OpType::Tmp
        || assign.op2 != sum.result
        || assign.result_type != OpType::Unused
        || !matches!(backedge.opcode, OpCode::Jmp | OpCode::QuickLongLoopJmp)
        || backedge.op1 as usize != header_ip
    {
        return None;
    }

    let value_cv = (next.extended_value & 0xffff) as u16;
    let accumulator_cv = if sum.op1 == value_cv && sum.op2 != value_cv {
        sum.op2
    } else if sum.op2 == value_cv && sum.op1 != value_cv {
        sum.op1
    } else {
        return None;
    };
    if assign.op1 != accumulator_cv || value_cv == accumulator_cv {
        return None;
    }

    let exit_ip = branch.op2 as usize;
    if exit_ip <= backedge_ip || exit_ip >= op_array.instructions.len() {
        return None;
    }

    let total_slots = op_array.num_cvs.checked_add(op_array.num_temps)?;
    let temporary_slots = [next.op1, next.op2, next.result, sum.result];
    if total_slots > 64
        || value_cv as u32 >= op_array.num_cvs
        || accumulator_cv as u32 >= op_array.num_cvs
        || temporary_slots
            .iter()
            .any(|slot| (*slot as u32) < op_array.num_cvs || (*slot as u32) >= total_slots)
        || temporary_slots.iter().enumerate().any(|(index, slot)| {
            temporary_slots[index + 1..].iter().any(|other| other == slot)
        })
    {
        return None;
    }

    Some(QuickForeachLongAccumulateLoop {
        header_ip,
        exit_ip,
        array_tmp: next.op1,
        position_tmp: next.op2,
        value_cv,
        done_tmp: next.result,
        accumulator_cv,
        sum_tmp: sum.result,
        sum_ip: header_ip + 2,
    })
}

/// Recognize a side-effect-free loop whose only body operation increments the
/// induction variable.
pub fn detect_long_induction_loop(
    op_array: &OpArray,
    header_ip: usize,
    backedge_ip: usize,
) -> Option<QuickLongInductionLoop> {
    if header_ip.checked_add(3)? != backedge_ip
        || backedge_ip >= op_array.instructions.len()
    {
        return None;
    }

    let condition = op_array.instructions[header_ip];
    let branch = op_array.instructions[header_ip + 1];
    let backedge = op_array.instructions[backedge_ip];
    let (induction_cv, bound, condition_tmp, exit_ip) = match condition.opcode {
        OpCode::IsSmaller
            if condition.op1_type == OpType::Cv
                && condition.op2_type == OpType::Cv
                && condition.result_type == OpType::Tmp
                && branch.opcode == OpCode::JmpZ
                && branch.op1_type == OpType::Tmp
                && branch.op1 == condition.result =>
        {
            (
                condition.op1,
                QuickLongBound::Cv(condition.op2),
                Some(condition.result),
                branch.op2 as usize,
            )
        }
        OpCode::IsSmaller_CvConst
            if condition.op1_type == OpType::Cv
                && condition.op2_type == OpType::Const
                && condition.result_type == OpType::Tmp
                && branch.opcode == OpCode::JmpZ
                && branch.op1_type == OpType::Tmp
                && branch.op1 == condition.result =>
        {
            (
                condition.op1,
                QuickLongBound::Const(long_literal(op_array, condition.op2)?),
                Some(condition.result),
                branch.op2 as usize,
            )
        }
        OpCode::JmpZ_Lt_CvConst
            if condition.op1_type == OpType::Cv
                && condition.op2_type == OpType::Const
                && condition.result_type == OpType::Unused
                && branch.opcode == OpCode::JmpZ
                && branch.op1_type == OpType::Tmp
                && branch.op2_type == OpType::Unused
                && branch.op2 == condition.result =>
        {
            (
                condition.op1,
                QuickLongBound::Const(long_literal(op_array, condition.op2)?),
                None,
                condition.result as usize,
            )
        }
        _ => return None,
    };

    if exit_ip <= backedge_ip || exit_ip >= op_array.instructions.len() {
        return None;
    }

    let increment_ip = header_ip + 2;
    let increment = op_array.instructions[increment_ip];
    let increment_kind = match increment.opcode {
        OpCode::PreInc => QuickIncrementKind::Pre,
        OpCode::PostInc => QuickIncrementKind::Post,
        _ => return None,
    };
    if increment.op1_type != OpType::Cv || increment.op1 != induction_cv {
        return None;
    }
    let increment_tmp = match increment.result_type {
        OpType::Unused => None,
        OpType::Tmp => Some(increment.result),
        _ => return None,
    };

    if !matches!(backedge.opcode, OpCode::Jmp | OpCode::QuickLongLoopJmp)
        || backedge.op1 as usize != header_ip
        || matches!(bound, QuickLongBound::Cv(cv) if cv == induction_cv)
    {
        return None;
    }

    if condition_tmp.is_some() && condition_tmp == increment_tmp {
        return None;
    }

    let total_slots = op_array.num_cvs.checked_add(op_array.num_temps)?;
    if total_slots > 64
        || induction_cv as u32 >= op_array.num_cvs
        || matches!(bound, QuickLongBound::Cv(cv) if cv as u32 >= op_array.num_cvs)
        || condition_tmp.is_some_and(|slot| slot as u32 >= total_slots)
        || increment_tmp.is_some_and(|slot| slot as u32 >= total_slots)
    {
        return None;
    }

    Some(QuickLongInductionLoop {
        header_ip,
        exit_ip,
        induction_cv,
        bound,
        condition_tmp,
        increment_kind,
        increment_tmp,
        increment_ip,
    })
}

fn detect_scalar_method_call_tree(
    op_array: &OpArray,
    initializer_ip: usize,
    total_slots: u32,
    long_input_mask: &mut u64,
    object_input_mask: &mut u64,
    depth: usize,
) -> Option<usize> {
    if depth >= 8 {
        return None;
    }
    let initializer = *op_array.instructions.get(initializer_ip)?;
    let (argument_count, argument_offset) = match initializer.opcode {
        OpCode::InitFcall => (initializer.op1 as usize, 0usize),
        OpCode::InitMethodCall if initializer.op1_type == OpType::Cv => {
            add_mask_slot(object_input_mask, initializer.op1, total_slots)?;
            (initializer.extended_value as usize, 1usize)
        }
        _ => return None,
    };
    if argument_count > 8 {
        return None;
    }

    let mut cursor = initializer_ip + 1;
    for argument_index in 0..argument_count {
        let instruction = *op_array.instructions.get(cursor)?;
        let destination = u16::try_from(argument_index.checked_add(argument_offset)?).ok()?;

        if matches!(instruction.opcode, OpCode::SendVal | OpCode::SendVarEx) {
            if instruction.op2 != destination {
                return None;
            }
            match instruction.op1_type {
                OpType::Cv => {
                    add_mask_slot(long_input_mask, instruction.op1, total_slots)?;
                }
                OpType::Const => {
                    long_literal(op_array, instruction.op1)?;
                }
                _ => return None,
            }
            cursor += 1;
            continue;
        }

        if matches!(
            instruction.opcode,
            OpCode::Add
                | OpCode::Add_CvTmp
                | OpCode::Add_TmpTmp
                | OpCode::Sub
                | OpCode::Sub_CvConst
                | OpCode::Sub_TmpTmp
                | OpCode::Mul
        ) {
            if !matches!(instruction.result_type, OpType::Tmp | OpType::Var) {
                return None;
            }
            for (op_type, operand) in [
                (instruction.op1_type, instruction.op1),
                (instruction.op2_type, instruction.op2),
            ] {
                match op_type {
                    OpType::Cv => {
                        add_mask_slot(long_input_mask, operand, total_slots)?;
                    }
                    OpType::Const => {
                        long_literal(op_array, operand)?;
                    }
                    _ => return None,
                }
            }
            let send = *op_array.instructions.get(cursor + 1)?;
            if !matches!(send.opcode, OpCode::SendVal | OpCode::SendVarEx)
                || !matches!(send.op1_type, OpType::Tmp | OpType::Var)
                || send.op1 != instruction.result
                || send.op2 != destination
            {
                return None;
            }
            cursor += 2;
            continue;
        }

        if matches!(instruction.opcode, OpCode::InitFcall | OpCode::InitMethodCall) {
            let nested_do_fcall_ip = detect_scalar_method_call_tree(
                op_array,
                cursor,
                total_slots,
                long_input_mask,
                object_input_mask,
                depth + 1,
            )?;
            let nested_do_fcall = *op_array.instructions.get(nested_do_fcall_ip)?;
            let send = *op_array.instructions.get(nested_do_fcall_ip + 1)?;
            if !matches!(nested_do_fcall.result_type, OpType::Tmp | OpType::Var)
                || !matches!(send.opcode, OpCode::SendVal | OpCode::SendVarEx)
                || !matches!(send.op1_type, OpType::Tmp | OpType::Var)
                || send.op1 != nested_do_fcall.result
                || send.op2 != destination
            {
                return None;
            }
            cursor = nested_do_fcall_ip + 2;
            continue;
        }

        return None;
    }

    let do_fcall = *op_array.instructions.get(cursor)?;
    (do_fcall.opcode == OpCode::DoFcall).then_some(cursor)
}

/// Recognize one side-effect-free scalar loop region.
///
/// `header_ip` is the target of the backward `Jmp`; `backedge_ip` is the
/// instruction position of that `Jmp`.
pub fn detect_long_accumulate_loop(
    op_array: &OpArray,
    header_ip: usize,
    backedge_ip: usize,
) -> Option<QuickLongAccumulateLoop> {
    if header_ip.checked_add(5)? > backedge_ip
        || header_ip.checked_add(23)? < backedge_ip
        || backedge_ip >= op_array.instructions.len()
    {
        return None;
    }

    let condition = op_array.instructions[header_ip];
    let branch = op_array.instructions[header_ip + 1];
    let backedge = op_array.instructions[backedge_ip];

    let (induction_cv, bound, condition_tmp, exit_ip) = match condition.opcode {
        OpCode::IsSmaller
            if condition.op1_type == OpType::Cv
                && condition.op2_type == OpType::Cv
                && condition.result_type == OpType::Tmp
                && branch.opcode == OpCode::JmpZ
                && branch.op1_type == OpType::Tmp
                && branch.op1 == condition.result =>
        {
            (
                condition.op1,
                QuickLongBound::Cv(condition.op2),
                Some(condition.result),
                branch.op2 as usize,
            )
        }
        OpCode::IsSmaller_CvConst
            if condition.op1_type == OpType::Cv
                && condition.op2_type == OpType::Const
                && condition.result_type == OpType::Tmp
                && branch.opcode == OpCode::JmpZ
                && branch.op1_type == OpType::Tmp
                && branch.op1 == condition.result =>
        {
            (
                condition.op1,
                QuickLongBound::Const(long_literal(op_array, condition.op2)?),
                Some(condition.result),
                branch.op2 as usize,
            )
        }
        OpCode::JmpZ_Lt_CvConst
            if condition.op1_type == OpType::Cv
                && condition.op2_type == OpType::Const
                && condition.result_type == OpType::Unused
                && branch.opcode == OpCode::JmpZ
                && branch.op1_type == OpType::Tmp
                && branch.op2_type == OpType::Unused
                && branch.op2 == condition.result =>
        {
            (
                condition.op1,
                QuickLongBound::Const(long_literal(op_array, condition.op2)?),
                None,
                condition.result as usize,
            )
        }
        _ => return None,
    };

    if exit_ip <= backedge_ip || exit_ip >= op_array.instructions.len() {
        return None;
    }

    let first_body = op_array.instructions[header_ip + 2];
    let scalar_call_shape = if first_body.opcode == OpCode::InitFcall {
        let argument_count = first_body.op1 as usize;
        if argument_count > 8 {
            return None;
        }
        let total_slots = op_array.num_cvs.checked_add(op_array.num_temps)?;
        if total_slots > 64 {
            return None;
        }
        let mut long_input_mask = 0u64;
        let mut operations = Vec::with_capacity(8);
        let mut arguments = [ScalarLongSource::Constant(0); 8];
        let mut produced_temporary_slots = [u16::MAX; 8];
        let mut expression_count = 0usize;
        let mut sent_arguments = 0usize;
        let mut do_fcall_ip = header_ip + 3;
        while sent_arguments < argument_count {
            let instruction = *op_array.instructions.get(do_fcall_ip)?;
            match instruction.opcode {
                OpCode::Add
                | OpCode::Add_CvTmp
                | OpCode::Add_TmpTmp
                | OpCode::Sub
                | OpCode::Sub_CvConst
                | OpCode::Sub_TmpTmp
                | OpCode::Mul => {
                    if expression_count == 8
                        || !matches!(instruction.result_type, OpType::Tmp | OpType::Var)
                    {
                        return None;
                    }
                    for (op_type, operand) in [
                        (instruction.op1_type, instruction.op1),
                        (instruction.op2_type, instruction.op2),
                    ] {
                        if op_type == OpType::Cv {
                            add_mask_slot(&mut long_input_mask, operand, total_slots)?;
                        }
                    }
                    let lhs = quick_scalar_long_source(
                        op_array,
                        instruction.op1_type,
                        instruction.op1,
                        &produced_temporary_slots,
                        expression_count,
                    )?;
                    let rhs = quick_scalar_long_source(
                        op_array,
                        instruction.op2_type,
                        instruction.op2,
                        &produced_temporary_slots,
                        expression_count,
                    )?;
                    if produced_temporary_slots[..expression_count]
                        .contains(&instruction.result)
                    {
                        return None;
                    }
                    let kind = match instruction.opcode {
                        OpCode::Add | OpCode::Add_CvTmp | OpCode::Add_TmpTmp => {
                            ScalarLongOpKind::Add
                        }
                        OpCode::Sub | OpCode::Sub_CvConst | OpCode::Sub_TmpTmp => {
                            ScalarLongOpKind::Subtract
                        }
                        OpCode::Mul => ScalarLongOpKind::Multiply,
                        _ => unreachable!(),
                    };
                    operations.push(ScalarLongOp {
                        kind,
                        lhs,
                        rhs,
                    });
                    produced_temporary_slots[expression_count] = instruction.result;
                    expression_count += 1;
                }
                OpCode::SendVal if instruction.op2 as usize == sent_arguments => {
                    if instruction.op1_type == OpType::Cv {
                        add_mask_slot(
                            &mut long_input_mask,
                            instruction.op1,
                            total_slots,
                        )?;
                    }
                    arguments[sent_arguments] = quick_scalar_long_source(
                        op_array,
                        instruction.op1_type,
                        instruction.op1,
                        &produced_temporary_slots,
                        expression_count,
                    )?;
                    sent_arguments += 1;
                }
                _ => return None,
            }
            do_fcall_ip += 1;
        }

        let do_fcall = op_array.instructions[do_fcall_ip];
        let sum_ip = do_fcall_ip + 1;
        let sum = op_array.instructions[sum_ip];
        if backedge_ip != do_fcall_ip + 4
            || do_fcall.opcode != OpCode::DoFcall
            || do_fcall.result_type != OpType::Tmp
            || sum.opcode != OpCode::Add_CvTmp
            || sum.op1_type != OpType::Cv
            || sum.op2_type != OpType::Tmp
            || sum.op2 != do_fcall.result
            || sum.result_type != OpType::Tmp
        {
            return None;
        }
        Some((
            sum.op1,
            QuickLongTerm::ScalarFunctionCall {
                guard: ScalarLongCallGuard::FunctionCache {
                    cache_ip: u32::try_from(header_ip + 2).ok()?,
                },
                do_fcall_ip,
                long_input_mask,
                argument_plan: Box::new(ScalarLongProgram {
                    operations: operations.into_boxed_slice(),
                    outputs: arguments,
                    output_count: argument_count as u8,
                }),
                argument_count: argument_count as u8,
                term_tmp: do_fcall.result,
            },
            sum.result,
            sum_ip,
            sum_ip + 1,
        ))
    } else if first_body.opcode == OpCode::InitMethodCall {
        let total_slots = op_array.num_cvs.checked_add(op_array.num_temps)?;
        if total_slots > 64 {
            return None;
        }
        let mut long_input_mask = 0u64;
        let mut object_input_mask = 0u64;
        let call_ip = header_ip + 2;
        let do_fcall_ip = detect_scalar_method_call_tree(
            op_array,
            call_ip,
            total_slots,
            &mut long_input_mask,
            &mut object_input_mask,
            0,
        )?;
        if long_input_mask & object_input_mask != 0 {
            return None;
        }
        let argument_count = u8::try_from(first_body.extended_value).ok()?;
        let do_fcall = op_array.instructions[do_fcall_ip];
        let sum_ip = do_fcall_ip + 1;
        let sum = *op_array.instructions.get(sum_ip)?;
        if backedge_ip != do_fcall_ip + 4
            || do_fcall.result_type != OpType::Tmp
            || sum.opcode != OpCode::Add_CvTmp
            || sum.op1_type != OpType::Cv
            || sum.op2_type != OpType::Tmp
            || sum.op2 != do_fcall.result
            || sum.result_type != OpType::Tmp
        {
            return None;
        }
        Some((
            sum.op1,
            QuickLongTerm::ScalarMethodCall {
                guard: ScalarLongCallGuard::MethodCache {
                    cache_ip: u32::try_from(call_ip).ok()?,
                    receiver_slot: first_body.op1,
                },
                do_fcall_ip,
                long_input_mask,
                object_input_mask,
                argument_count,
                term_tmp: do_fcall.result,
            },
            sum.result,
            sum_ip,
            sum_ip + 1,
        ))
    } else {
        None
    };
    let (accumulator_cv, term, sum_tmp, sum_ip, assign_ip) = if let Some(shape) = scalar_call_shape {
        shape
    } else if backedge_ip == header_ip + 5 {
        if first_body.opcode != OpCode::Add
            || first_body.op1_type != OpType::Cv
            || first_body.op2_type != OpType::Cv
            || first_body.op2 != induction_cv
            || first_body.result_type != OpType::Tmp
        {
            return None;
        }
        (
            first_body.op1,
            QuickLongTerm::Induction,
            first_body.result,
            header_ip + 2,
            header_ip + 3,
        )
    } else if backedge_ip == header_ip + 6 {
        let sum = op_array.instructions[header_ip + 3];
        if first_body.result_type != OpType::Tmp
            || sum.opcode != OpCode::Add_CvTmp
            || sum.op1_type != OpType::Cv
            || sum.op2_type != OpType::Tmp
            || sum.op2 != first_body.result
            || sum.result_type != OpType::Tmp
        {
            return None;
        }
        let term = match (first_body.opcode, first_body.op1_type, first_body.op2_type) {
            (OpCode::Add, OpType::Cv, OpType::Const) if first_body.op1 == induction_cv => {
                QuickLongTerm::InductionPlusConst {
                    addend: long_literal(op_array, first_body.op2)?,
                    term_tmp: first_body.result,
                    term_ip: header_ip + 2,
                }
            }
            (OpCode::Add, OpType::Const, OpType::Cv) if first_body.op2 == induction_cv => {
                QuickLongTerm::InductionPlusConst {
                    addend: long_literal(op_array, first_body.op1)?,
                    term_tmp: first_body.result,
                    term_ip: header_ip + 2,
                }
            }
            (OpCode::Add, OpType::Cv, OpType::Cv)
                if first_body.op1 == induction_cv && first_body.op2 != induction_cv =>
            {
                QuickLongTerm::InductionPlusCv {
                    addend_cv: first_body.op2,
                    term_tmp: first_body.result,
                    term_ip: header_ip + 2,
                }
            }
            (OpCode::Add, OpType::Cv, OpType::Cv)
                if first_body.op2 == induction_cv && first_body.op1 != induction_cv =>
            {
                QuickLongTerm::InductionPlusCv {
                    addend_cv: first_body.op1,
                    term_tmp: first_body.result,
                    term_ip: header_ip + 2,
                }
            }
            (OpCode::FetchDimR, OpType::Cv, OpType::Cv) => {
                QuickLongTerm::ArrayIndex {
                    array_cv: first_body.op1,
                    index: if first_body.op2 == induction_cv {
                        QuickArrayIndex::Long(QuickLongOperand::Slot(induction_cv))
                    } else {
                        QuickArrayIndex::ValueSlot(first_body.op2)
                    },
                    term_tmp: first_body.result,
                    destination: None,
                    fetch_ip: header_ip + 2,
                }
            }
            (OpCode::FetchDimR, OpType::Cv, OpType::Const) => {
                QuickLongTerm::ArrayIndex {
                    array_cv: first_body.op1,
                    index: array_literal_index(op_array, first_body.op2)?,
                    term_tmp: first_body.result,
                    destination: None,
                    fetch_ip: header_ip + 2,
                }
            }
            (OpCode::Strlen_Cv, OpType::Cv, OpType::Unused) => {
                QuickLongTerm::StringLength {
                    string_cv: first_body.op1,
                    term_tmp: first_body.result,
                    term_ip: header_ip + 2,
                }
            }
            (OpCode::DirectInternalCall1, OpType::Cv, OpType::Unused)
                if crate::builtin_metadata::DirectInternalKind::from_id(
                    first_body.extended_value,
                ) == Some(crate::builtin_metadata::DirectInternalKind::Abs) =>
            {
                QuickLongTerm::AbsLong {
                    operand_cv: first_body.op1,
                    term_tmp: first_body.result,
                    term_ip: header_ip + 2,
                }
            }
            _ => return None,
        };
        (sum.op1, term, sum.result, header_ip + 3, header_ip + 4)
    } else {
        let materialize = op_array.instructions[header_ip + 3];
        let sum = op_array.instructions[header_ip + 4];
        if first_body.opcode != OpCode::FetchDimR
            || first_body.op1_type != OpType::Cv
            || !matches!(first_body.op2_type, OpType::Cv | OpType::Const)
            || first_body.result_type != OpType::Tmp
            || materialize.opcode != OpCode::AssignCv
            || materialize.op1_type != OpType::Cv
            || materialize.op2_type != OpType::Tmp
            || materialize.op2 != first_body.result
            || materialize.result_type != OpType::Unused
            || sum.opcode != OpCode::Add
            || sum.op1_type != OpType::Cv
            || sum.op2_type != OpType::Cv
            || sum.result_type != OpType::Tmp
        {
            return None;
        }
        let accumulator_cv = if sum.op1 == materialize.op1 && sum.op2 != materialize.op1 {
            sum.op2
        } else if sum.op2 == materialize.op1 && sum.op1 != materialize.op1 {
            sum.op1
        } else {
            return None;
        };
        (
            accumulator_cv,
            QuickLongTerm::ArrayIndex {
                array_cv: first_body.op1,
                index: match first_body.op2_type {
                    OpType::Cv => QuickArrayIndex::ValueSlot(first_body.op2),
                    OpType::Const => array_literal_index(op_array, first_body.op2)?,
                    _ => unreachable!(),
                },
                term_tmp: first_body.result,
                destination: Some(materialize.op1),
                fetch_ip: header_ip + 2,
            },
            sum.result,
            header_ip + 4,
            header_ip + 5,
        )
    };

    let assign = op_array.instructions[assign_ip];
    if assign.opcode != OpCode::AssignCv
        || assign.op1_type != OpType::Cv
        || assign.op1 != accumulator_cv
        || assign.op2_type != OpType::Tmp
        || assign.op2 != sum_tmp
        || assign.result_type != OpType::Unused
    {
        return None;
    }

    let increment_ip = assign_ip + 1;
    let increment = op_array.instructions[increment_ip];
    let increment_kind = match increment.opcode {
        OpCode::PreInc => QuickIncrementKind::Pre,
        OpCode::PostInc => QuickIncrementKind::Post,
        _ => return None,
    };
    if increment.op1_type != OpType::Cv
        || increment.op1 != induction_cv
    {
        return None;
    }
    let increment_tmp = match increment.result_type {
        OpType::Unused => None,
        OpType::Tmp => Some(increment.result),
        _ => return None,
    };

    if !matches!(backedge.opcode, OpCode::Jmp | OpCode::QuickLongLoopJmp)
        || backedge.op1 as usize != header_ip
    {
        return None;
    }

    if accumulator_cv == induction_cv
        || matches!(bound, QuickLongBound::Cv(cv) if cv == induction_cv || cv == accumulator_cv)
        || matches!(
            term,
            QuickLongTerm::InductionPlusCv { addend_cv, .. }
                if addend_cv == induction_cv || addend_cv == accumulator_cv
        )
        || matches!(
            term,
            QuickLongTerm::ArrayIndex { array_cv, .. }
                if array_cv == induction_cv || array_cv == accumulator_cv
        )
        || matches!(
            term,
            QuickLongTerm::StringLength { string_cv, .. }
                if string_cv == induction_cv || string_cv == accumulator_cv
        )
        || matches!(
            term,
            QuickLongTerm::AbsLong { operand_cv, .. }
                if operand_cv == accumulator_cv
        )
        || matches!(
            term,
            QuickLongTerm::ArrayIndex {
                array_cv,
                index: QuickArrayIndex::ValueSlot(index_cv),
                destination,
                ..
            } if index_cv == induction_cv
                || index_cv == accumulator_cv
                || index_cv == array_cv
                || destination == Some(index_cv)
                || matches!(bound, QuickLongBound::Cv(bound_cv) if index_cv == bound_cv)
        )
        || matches!(
            term,
            QuickLongTerm::ArrayIndex {
                array_cv,
                destination: Some(destination),
                ..
            } if destination == induction_cv
                || destination == accumulator_cv
                || destination == array_cv
                || matches!(bound, QuickLongBound::Cv(bound_cv) if destination == bound_cv)
        )
    {
        return None;
    }

    let mut temporary_slots = vec![sum_tmp];
    if let Some(slot) = condition_tmp {
        temporary_slots.push(slot);
    }
    match term {
        QuickLongTerm::Induction => {}
        QuickLongTerm::InductionPlusConst { term_tmp, .. }
        | QuickLongTerm::InductionPlusCv { term_tmp, .. }
        | QuickLongTerm::ArrayIndex { term_tmp, .. }
        | QuickLongTerm::StringLength { term_tmp, .. }
        | QuickLongTerm::AbsLong { term_tmp, .. }
        | QuickLongTerm::ScalarFunctionCall { term_tmp, .. }
        | QuickLongTerm::ScalarMethodCall { term_tmp, .. } => {
            temporary_slots.push(term_tmp);
        }
    }
    if let Some(slot) = increment_tmp {
        temporary_slots.push(slot);
    }
    let temporary_slot_count = temporary_slots.len();
    temporary_slots.sort_unstable();
    temporary_slots.dedup();
    if temporary_slots.len() != temporary_slot_count {
        return None;
    }

    let total_slots = op_array.num_cvs.checked_add(op_array.num_temps)?;
    if total_slots > 64
        || temporary_slots
            .iter()
            .any(|slot| *slot as u32 >= total_slots)
        || induction_cv as u32 >= op_array.num_cvs
        || accumulator_cv as u32 >= op_array.num_cvs
        || matches!(bound, QuickLongBound::Cv(cv) if cv as u32 >= op_array.num_cvs)
        || matches!(
            term,
            QuickLongTerm::InductionPlusCv { addend_cv, .. }
                if addend_cv as u32 >= op_array.num_cvs
        )
        || matches!(
            term,
            QuickLongTerm::ArrayIndex { array_cv, .. }
                if array_cv as u32 >= op_array.num_cvs
        )
        || matches!(
            term,
            QuickLongTerm::StringLength { string_cv, .. }
                if string_cv as u32 >= op_array.num_cvs
        )
        || matches!(
            term,
            QuickLongTerm::AbsLong { operand_cv, .. }
                if operand_cv as u32 >= op_array.num_cvs
        )
        || matches!(
            term,
            QuickLongTerm::ArrayIndex {
                index: QuickArrayIndex::ValueSlot(index_cv),
                ..
            } if index_cv as u32 >= op_array.num_cvs
        )
        || matches!(
            term,
            QuickLongTerm::ArrayIndex {
                destination: Some(destination),
                ..
            } if destination as u32 >= op_array.num_cvs
        )
    {
        return None;
    }

    Some(QuickLongAccumulateLoop {
        header_ip,
        exit_ip,
        induction_cv,
        bound,
        accumulator_cv,
        condition_tmp,
        term,
        sum_tmp,
        increment_kind,
        increment_tmp,
        sum_ip,
        increment_ip,
    })
}

fn add_mask_slot(mask: &mut u64, slot: u16, total_slots: u32) -> Option<()> {
    if slot as u32 >= total_slots || slot >= 64 {
        return None;
    }
    *mask |= 1u64 << slot;
    Some(())
}

fn long_slot(op_type: OpType, slot: u16) -> Option<u16> {
    matches!(op_type, OpType::Cv | OpType::Tmp).then_some(slot)
}

fn instruction_writes_cv(instruction: crate::vm::instruction::Instruction, cv: u16) -> bool {
    if instruction.result_type == OpType::Cv && instruction.result == cv {
        return true;
    }
    if instruction.opcode == OpCode::ForeachNext {
        let value_cv = instruction.extended_value as u16;
        let encoded_key = (instruction.extended_value >> 16) as u16;
        return value_cv == cv || (encoded_key != 0 && encoded_key - 1 == cv);
    }
    matches!(
        instruction.opcode,
        OpCode::AssignCv
            | OpCode::AssignConcat
            | OpCode::PreInc
            | OpCode::PreDec
            | OpCode::PostInc
            | OpCode::PostDec
            | OpCode::BindGlobal
            | OpCode::BindStatic
    ) && instruction.op1_type == OpType::Cv
        && instruction.op1 == cv
}

fn preheader_string_literal_cv(op_array: &OpArray, header_ip: usize, cv: u16) -> bool {
    op_array.instructions[..header_ip]
        .iter()
        .rev()
        .copied()
        .find(|instruction| instruction_writes_cv(*instruction, cv))
        .is_some_and(|instruction| {
            instruction.opcode == OpCode::AssignCv
                && instruction.op1_type == OpType::Cv
                && instruction.op2_type == OpType::Const
                && instruction.result_type == OpType::Unused
                && op_array
                    .literals
                    .get(instruction.op2 as usize)
                    .and_then(Value::as_str)
                    .is_some()
        })
}

fn cv_unmodified_in_region(
    instructions: &[crate::vm::instruction::Instruction],
    cv: u16,
) -> bool {
    instructions
        .iter()
        .copied()
        .all(|instruction| !instruction_writes_cv(instruction, cv))
}

fn long_add(instruction: crate::vm::instruction::Instruction) -> Option<(u16, u16, u16)> {
    if !matches!(
        instruction.opcode,
        OpCode::Add | OpCode::Add_CvTmp | OpCode::Add_TmpTmp
    ) || instruction.result_type != OpType::Tmp
    {
        return None;
    }
    Some((
        long_slot(instruction.op1_type, instruction.op1)?,
        long_slot(instruction.op2_type, instruction.op2)?,
        instruction.result,
    ))
}

fn long_assign(instruction: crate::vm::instruction::Instruction) -> Option<(u16, u16)> {
    if instruction.opcode != OpCode::AssignCv
        || instruction.op1_type != OpType::Cv
        || instruction.result_type != OpType::Unused
    {
        return None;
    }
    Some((
        instruction.op1,
        long_slot(instruction.op2_type, instruction.op2)?,
    ))
}

fn conditional_add_assign(
    op_array: &OpArray,
    add_ip: usize,
    false_ip: usize,
) -> Option<(u16, u16, u16, u16, usize)> {
    let (lhs, rhs, result) = long_add(*op_array.instructions.get(add_ip)?)?;
    let (destination, source) = long_assign(*op_array.instructions.get(add_ip + 1)?)?;
    let next_ip = add_ip + 2;
    (source == result && false_ip == next_ip).then_some((lhs, rhs, result, destination, next_ip))
}

/// Build a small typed program for a closed scalar loop.
///
/// This deliberately supports only side-effect-free long operations and
/// forward branches inside the body. Unsupported instructions leave the
/// original backedge untouched.
pub fn detect_long_ops_loop(
    op_array: &OpArray,
    header_ip: usize,
    backedge_ip: usize,
) -> Option<QuickLongOpsLoop> {
    if header_ip >= backedge_ip
        || backedge_ip >= op_array.instructions.len()
        || backedge_ip - header_ip >= u16::MAX as usize
    {
        return None;
    }

    let backedge = op_array.instructions[backedge_ip];
    if !matches!(backedge.opcode, OpCode::Jmp | OpCode::QuickLongLoopJmp)
        || backedge.op1 as usize != header_ip
    {
        return None;
    }

    let total_slots = op_array.num_cvs.checked_add(op_array.num_temps)?;
    if total_slots > 64 {
        return None;
    }

    let region_len = backedge_ip - header_ip + 1;
    let mut ip_to_op = vec![u16::MAX; region_len];
    let mut ops = Vec::new();
    let mut op_ips = Vec::new();
    let mut long_input_mask = 0u64;
    let mut long_output_mask = 0u64;
    let mut bool_output_mask = 0u64;
    let mut array_input_mask = 0u64;
    let mut string_input_mask = 0u64;
    let mut string_output_mask = 0u64;
    let mut object_input_mask = 0u64;
    let mut has_add = false;
    let mut has_assign = false;
    let mut has_object_call = false;
    let mut has_post_inc = false;
    let mut ip = header_ip;

    // String array-key state is selected only when every visible assignment to
    // that key is either a string literal or an immutable CV whose last
    // preheader definition is a string literal. Runtime guards still verify all
    // selected CVs; ambiguous integer-key programs keep their established path.
    let region = &op_array.instructions[header_ip..=backedge_ip];
    let mut array_index_cv_mask = 0u64;
    for instruction in region {
        if instruction.opcode == OpCode::FetchDimR && instruction.op2_type == OpType::Cv {
            add_mask_slot(&mut array_index_cv_mask, instruction.op2, total_slots)?;
        }
    }

    let mut string_key_assignment_mask = 0u64;
    let mut candidates = array_index_cv_mask;
    while candidates != 0 {
        let key = candidates.trailing_zeros() as u16;
        candidates &= candidates - 1;
        let mut has_assignment = false;
        let mut valid = true;
        for instruction in region.iter().filter(|instruction| {
            instruction.opcode == OpCode::AssignCv
                && instruction.op1_type == OpType::Cv
                && instruction.op1 == key
                && instruction.result_type == OpType::Unused
        }) {
            has_assignment = true;
            valid &= match instruction.op2_type {
                OpType::Const => op_array
                    .literals
                    .get(instruction.op2 as usize)
                    .and_then(Value::as_str)
                    .is_some(),
                OpType::Cv => {
                    preheader_string_literal_cv(op_array, header_ip, instruction.op2)
                        && cv_unmodified_in_region(region, instruction.op2)
                }
                _ => false,
            };
        }
        if has_assignment && valid {
            add_mask_slot(&mut string_key_assignment_mask, key, total_slots)?;
        }
    }

    let mut string_source_input_mask = 0u64;
    let mut string_cache_literals = [u16::MAX; QUICK_STRING_FETCH_CACHE_LIMIT];
    let mut string_cache_literal_count = 0usize;
    for instruction in region.iter().filter(|instruction| {
        instruction.opcode == OpCode::AssignCv
            && instruction.op1_type == OpType::Cv
            && string_key_assignment_mask & (1u64 << instruction.op1) != 0
    }) {
        match instruction.op2_type {
            OpType::Cv => {
                add_mask_slot(&mut string_source_input_mask, instruction.op2, total_slots)?;
            }
            OpType::Const
                if !string_cache_literals[..string_cache_literal_count]
                    .contains(&instruction.op2)
                    && string_cache_literal_count < string_cache_literals.len() =>
            {
                string_cache_literals[string_cache_literal_count] = instruction.op2;
                string_cache_literal_count += 1;
            }
            _ => {}
        }
    }
    string_input_mask |= string_source_input_mask;
    let string_cache_capacity = (string_source_input_mask.count_ones() as usize
        + string_cache_literal_count)
        .min(QUICK_STRING_FETCH_CACHE_LIMIT);

    while ip <= backedge_ip {
        let instruction = op_array.instructions[ip];
        let op = match instruction.opcode {
            OpCode::IsSmaller => {
                let branch = *op_array.instructions.get(ip + 1)?;
                if instruction.op1_type != OpType::Cv
                    || instruction.op2_type != OpType::Cv
                    || instruction.result_type != OpType::Tmp
                    || branch.opcode != OpCode::JmpZ
                    || branch.op1_type != OpType::Tmp
                    || branch.op1 != instruction.result
                    || branch.op2_type != OpType::Unused
                {
                    return None;
                }
                add_mask_slot(&mut long_input_mask, instruction.op1, total_slots)?;
                add_mask_slot(&mut long_input_mask, instruction.op2, total_slots)?;
                add_mask_slot(&mut bool_output_mask, instruction.result, total_slots)?;
                if let Some((lhs, rhs, result, destination, next_ip)) =
                    conditional_add_assign(op_array, ip + 2, branch.op2 as usize)
                {
                    add_mask_slot(&mut long_input_mask, lhs, total_slots)?;
                    add_mask_slot(&mut long_input_mask, rhs, total_slots)?;
                    add_mask_slot(&mut long_output_mask, result, total_slots)?;
                    add_mask_slot(&mut long_output_mask, destination, total_slots)?;
                    has_add = true;
                    has_assign = true;
                    let condition_resume_ip = ip;
                    ip += 4;
                    QuickLongOp::ConditionalAddAssign {
                        condition: QuickLongCondition::Lt {
                            lhs: instruction.op1,
                            rhs: QuickLongOperand::Slot(instruction.op2),
                        },
                        condition_tmp: Some(instruction.result),
                        lhs,
                        rhs,
                        result,
                        destination,
                        next_target: QuickLongTarget::unresolved(next_ip)?,
                        condition_resume_ip,
                        add_resume_ip: condition_resume_ip + 2,
                    }
                } else {
                    let op = QuickLongOp::BranchUnlessLt {
                        lhs: instruction.op1,
                        rhs: QuickLongOperand::Slot(instruction.op2),
                        condition_tmp: Some(instruction.result),
                        false_target: QuickLongTarget::unresolved(branch.op2 as usize)?,
                        next_target: QuickLongTarget::unresolved(ip + 2)?,
                        resume_ip: ip,
                    };
                    ip += 2;
                    op
                }
            }
            OpCode::IsSmaller_CvConst => {
                let branch = *op_array.instructions.get(ip + 1)?;
                if instruction.op1_type != OpType::Cv
                    || instruction.op2_type != OpType::Const
                    || instruction.result_type != OpType::Tmp
                    || branch.opcode != OpCode::JmpZ
                    || branch.op1_type != OpType::Tmp
                    || branch.op1 != instruction.result
                    || branch.op2_type != OpType::Unused
                {
                    return None;
                }
                add_mask_slot(&mut long_input_mask, instruction.op1, total_slots)?;
                add_mask_slot(&mut bool_output_mask, instruction.result, total_slots)?;
                let condition_rhs =
                    QuickLongOperand::Const(long_literal(op_array, instruction.op2)?);
                if let Some((lhs, rhs, result, destination, next_ip)) =
                    conditional_add_assign(op_array, ip + 2, branch.op2 as usize)
                {
                    add_mask_slot(&mut long_input_mask, lhs, total_slots)?;
                    add_mask_slot(&mut long_input_mask, rhs, total_slots)?;
                    add_mask_slot(&mut long_output_mask, result, total_slots)?;
                    add_mask_slot(&mut long_output_mask, destination, total_slots)?;
                    has_add = true;
                    has_assign = true;
                    let condition_resume_ip = ip;
                    ip += 4;
                    QuickLongOp::ConditionalAddAssign {
                        condition: QuickLongCondition::Lt {
                            lhs: instruction.op1,
                            rhs: condition_rhs,
                        },
                        condition_tmp: Some(instruction.result),
                        lhs,
                        rhs,
                        result,
                        destination,
                        next_target: QuickLongTarget::unresolved(next_ip)?,
                        condition_resume_ip,
                        add_resume_ip: condition_resume_ip + 2,
                    }
                } else {
                    let op = QuickLongOp::BranchUnlessLt {
                        lhs: instruction.op1,
                        rhs: condition_rhs,
                        condition_tmp: Some(instruction.result),
                        false_target: QuickLongTarget::unresolved(branch.op2 as usize)?,
                        next_target: QuickLongTarget::unresolved(ip + 2)?,
                        resume_ip: ip,
                    };
                    ip += 2;
                    op
                }
            }
            OpCode::JmpZ_Lt_CvConst => {
                let dead_branch = *op_array.instructions.get(ip + 1)?;
                if instruction.op1_type != OpType::Cv
                    || instruction.op2_type != OpType::Const
                    || instruction.result_type != OpType::Unused
                    || dead_branch.opcode != OpCode::JmpZ
                    || dead_branch.op1_type != OpType::Tmp
                    || dead_branch.op2_type != OpType::Unused
                    || dead_branch.op2 != instruction.result
                {
                    return None;
                }
                add_mask_slot(&mut long_input_mask, instruction.op1, total_slots)?;
                let condition_rhs =
                    QuickLongOperand::Const(long_literal(op_array, instruction.op2)?);
                if let Some((lhs, rhs, result, destination, next_ip)) =
                    conditional_add_assign(op_array, ip + 2, instruction.result as usize)
                {
                    add_mask_slot(&mut long_input_mask, lhs, total_slots)?;
                    add_mask_slot(&mut long_input_mask, rhs, total_slots)?;
                    add_mask_slot(&mut long_output_mask, result, total_slots)?;
                    add_mask_slot(&mut long_output_mask, destination, total_slots)?;
                    has_add = true;
                    has_assign = true;
                    let condition_resume_ip = ip;
                    ip += 4;
                    QuickLongOp::ConditionalAddAssign {
                        condition: QuickLongCondition::Lt {
                            lhs: instruction.op1,
                            rhs: condition_rhs,
                        },
                        condition_tmp: None,
                        lhs,
                        rhs,
                        result,
                        destination,
                        next_target: QuickLongTarget::unresolved(next_ip)?,
                        condition_resume_ip,
                        add_resume_ip: condition_resume_ip + 2,
                    }
                } else {
                    let op = QuickLongOp::BranchUnlessLt {
                        lhs: instruction.op1,
                        rhs: condition_rhs,
                        condition_tmp: None,
                        false_target: QuickLongTarget::unresolved(instruction.result as usize)?,
                        next_target: QuickLongTarget::unresolved(ip + 2)?,
                        resume_ip: ip,
                    };
                    ip += 2;
                    op
                }
            }
            OpCode::Mod => {
                let value = long_slot(instruction.op1_type, instruction.op1)?;
                if instruction.op2_type != OpType::Const || instruction.result_type != OpType::Tmp {
                    return None;
                }
                let divisor = long_literal(op_array, instruction.op2)?;
                if divisor == 0 {
                    return None;
                }
                add_mask_slot(&mut long_input_mask, value, total_slots)?;
                add_mask_slot(&mut long_output_mask, instruction.result, total_slots)?;
                let resume_ip = ip;
                ip += 1;
                QuickLongOp::ModConst {
                    value,
                    divisor,
                    result: instruction.result,
                    next_target: QuickLongTarget::unresolved(ip)?,
                    resume_ip,
                }
            }
            OpCode::FetchDimR => {
                let array = long_slot(instruction.op1_type, instruction.op1)?;
                if instruction.result_type != OpType::Tmp {
                    return None;
                }
                let index = match instruction.op2_type {
                    OpType::Cv | OpType::Tmp => {
                        if instruction.op2_type == OpType::Cv
                            && string_key_assignment_mask & (1u64 << instruction.op2) != 0
                        {
                            add_mask_slot(&mut string_input_mask, instruction.op2, total_slots)?;
                            QuickArrayIndex::ValueSlot(instruction.op2)
                        } else {
                            add_mask_slot(&mut long_input_mask, instruction.op2, total_slots)?;
                            QuickArrayIndex::Long(QuickLongOperand::Slot(instruction.op2))
                        }
                    }
                    OpType::Const => array_literal_index(op_array, instruction.op2)?,
                    _ => return None,
                };
                add_mask_slot(&mut array_input_mask, array, total_slots)?;
                add_mask_slot(&mut long_output_mask, instruction.result, total_slots)?;
                let destination = op_array
                    .instructions
                    .get(ip + 1)
                    .copied()
                    .and_then(long_assign)
                    .and_then(|(destination, source)| {
                        (source == instruction.result).then_some(destination)
                    });
                let resume_ip = ip;
                if let Some(destination) = destination {
                    add_mask_slot(&mut long_output_mask, destination, total_slots)?;
                    has_assign = true;
                    ip += 2;
                } else {
                    ip += 1;
                }
                QuickLongOp::FetchArrayLong {
                    array,
                    index,
                    result: instruction.result,
                    destination,
                    next_target: QuickLongTarget::unresolved(ip)?,
                    resume_ip,
                }
            }
            OpCode::IsEqual | OpCode::IsEqual_CvConst => {
                let lhs = long_slot(instruction.op1_type, instruction.op1)?;
                let rhs = match instruction.op2_type {
                    OpType::Cv | OpType::Tmp => QuickLongOperand::Slot(instruction.op2),
                    OpType::Const => {
                        QuickLongOperand::Const(long_literal(op_array, instruction.op2)?)
                    }
                    _ => return None,
                };
                let branch = *op_array.instructions.get(ip + 1)?;
                if instruction.result_type != OpType::Tmp
                    || branch.opcode != OpCode::JmpZ
                    || branch.op1_type != OpType::Tmp
                    || branch.op1 != instruction.result
                    || branch.op2_type != OpType::Unused
                {
                    return None;
                }
                add_mask_slot(&mut long_input_mask, lhs, total_slots)?;
                if let QuickLongOperand::Slot(rhs) = rhs {
                    add_mask_slot(&mut long_input_mask, rhs, total_slots)?;
                }
                add_mask_slot(&mut bool_output_mask, instruction.result, total_slots)?;

                if let Some((add_lhs, add_rhs, result, destination, next_ip)) =
                    conditional_add_assign(op_array, ip + 2, branch.op2 as usize)
                {
                    add_mask_slot(&mut long_input_mask, add_lhs, total_slots)?;
                    add_mask_slot(&mut long_input_mask, add_rhs, total_slots)?;
                    add_mask_slot(&mut long_output_mask, result, total_slots)?;
                    add_mask_slot(&mut long_output_mask, destination, total_slots)?;
                    has_add = true;
                    has_assign = true;
                    let condition_resume_ip = ip;
                    ip += 4;
                    QuickLongOp::ConditionalAddAssign {
                        condition: QuickLongCondition::Eq { lhs, rhs },
                        condition_tmp: Some(instruction.result),
                        lhs: add_lhs,
                        rhs: add_rhs,
                        result,
                        destination,
                        next_target: QuickLongTarget::unresolved(next_ip)?,
                        condition_resume_ip,
                        add_resume_ip: condition_resume_ip + 2,
                    }
                } else {
                    let op = QuickLongOp::BranchUnlessEq {
                        lhs,
                        rhs,
                        condition_tmp: Some(instruction.result),
                        false_target: QuickLongTarget::unresolved(branch.op2 as usize)?,
                        next_target: QuickLongTarget::unresolved(ip + 2)?,
                        resume_ip: ip,
                    };
                    ip += 2;
                    op
                }
            }
            OpCode::JmpZ_Eq_CvConst => {
                let dead_branch = *op_array.instructions.get(ip + 1)?;
                if instruction.op1_type != OpType::Cv
                    || instruction.op2_type != OpType::Const
                    || instruction.result_type != OpType::Unused
                    || dead_branch.opcode != OpCode::JmpZ
                    || dead_branch.op1_type != OpType::Tmp
                    || dead_branch.op2_type != OpType::Unused
                    || dead_branch.op2 != instruction.result
                {
                    return None;
                }
                let condition_rhs =
                    QuickLongOperand::Const(long_literal(op_array, instruction.op2)?);
                add_mask_slot(&mut long_input_mask, instruction.op1, total_slots)?;

                if let Some((lhs, rhs, result, destination, next_ip)) =
                    conditional_add_assign(op_array, ip + 2, instruction.result as usize)
                {
                    add_mask_slot(&mut long_input_mask, lhs, total_slots)?;
                    add_mask_slot(&mut long_input_mask, rhs, total_slots)?;
                    add_mask_slot(&mut long_output_mask, result, total_slots)?;
                    add_mask_slot(&mut long_output_mask, destination, total_slots)?;
                    has_add = true;
                    has_assign = true;
                    let condition_resume_ip = ip;
                    ip += 4;
                    QuickLongOp::ConditionalAddAssign {
                        condition: QuickLongCondition::Eq {
                            lhs: instruction.op1,
                            rhs: condition_rhs,
                        },
                        condition_tmp: None,
                        lhs,
                        rhs,
                        result,
                        destination,
                        next_target: QuickLongTarget::unresolved(next_ip)?,
                        condition_resume_ip,
                        add_resume_ip: condition_resume_ip + 2,
                    }
                } else {
                    let op = QuickLongOp::BranchUnlessEq {
                        lhs: instruction.op1,
                        rhs: condition_rhs,
                        condition_tmp: None,
                        false_target: QuickLongTarget::unresolved(instruction.result as usize)?,
                        next_target: QuickLongTarget::unresolved(ip + 2)?,
                        resume_ip: ip,
                    };
                    ip += 2;
                    op
                }
            }
            OpCode::Add | OpCode::Add_CvTmp | OpCode::Add_TmpTmp => {
                let (lhs, rhs, result) = long_add(instruction)?;
                add_mask_slot(&mut long_input_mask, lhs, total_slots)?;
                add_mask_slot(&mut long_input_mask, rhs, total_slots)?;
                add_mask_slot(&mut long_output_mask, result, total_slots)?;
                has_add = true;

                if let (
                    Some((second_lhs, second_rhs, second_result)),
                    Some((destination, source)),
                ) = (
                    op_array
                        .instructions
                        .get(ip + 1)
                        .copied()
                        .and_then(long_add),
                    op_array
                        .instructions
                        .get(ip + 2)
                        .copied()
                        .and_then(long_assign),
                ) {
                    if source != second_result {
                        return None;
                    }
                    for input in [second_lhs, second_rhs] {
                        if input != result {
                            add_mask_slot(&mut long_input_mask, input, total_slots)?;
                        }
                    }
                    add_mask_slot(&mut long_output_mask, second_result, total_slots)?;
                    add_mask_slot(&mut long_output_mask, destination, total_slots)?;
                    has_assign = true;
                    let first_resume_ip = ip;
                    ip += 3;
                    QuickLongOp::AddAddAssign {
                        first_lhs: lhs,
                        first_rhs: rhs,
                        first_result: result,
                        second_lhs,
                        second_rhs,
                        second_result,
                        destination,
                        next_target: QuickLongTarget::unresolved(ip)?,
                        first_resume_ip,
                        second_resume_ip: first_resume_ip + 1,
                    }
                } else if let Some((destination, source)) = op_array
                    .instructions
                    .get(ip + 1)
                    .copied()
                    .and_then(long_assign)
                {
                    if source != result {
                        return None;
                    }
                    add_mask_slot(&mut long_output_mask, destination, total_slots)?;
                    has_assign = true;
                    let add_resume_ip = ip;
                    ip += 2;
                    QuickLongOp::AddAssign {
                        lhs,
                        rhs,
                        result,
                        destination,
                        next_target: QuickLongTarget::unresolved(ip)?,
                        add_resume_ip,
                    }
                } else {
                    ip += 1;
                    QuickLongOp::Add {
                        lhs,
                        rhs,
                        result,
                        next_target: QuickLongTarget::unresolved(ip)?,
                        resume_ip: ip - 1,
                    }
                }
            }
            OpCode::InitMethodCall => {
                if instruction.op1_type != OpType::Cv {
                    return None;
                }
                let cache_ip = u32::try_from(ip).ok()?;
                let outer_guard = ScalarLongCallGuard::MethodCache {
                    cache_ip,
                    receiver_slot: instruction.op1,
                };
                add_mask_slot(&mut object_input_mask, instruction.op1, total_slots)?;

                let nested = if instruction.extended_value == 1 {
                    let inner_init = *op_array.instructions.get(ip + 1)?;
                    let inner_do = *op_array.instructions.get(ip + 2)?;
                    let send = *op_array.instructions.get(ip + 3)?;
                    let outer_do = *op_array.instructions.get(ip + 4)?;
                    if inner_init.opcode == OpCode::InitMethodCall
                        && inner_init.op1_type == OpType::Cv
                        && inner_init.extended_value == 0
                        && inner_do.opcode == OpCode::DoFcall
                        && matches!(inner_do.result_type, OpType::Tmp | OpType::Var)
                        && matches!(send.opcode, OpCode::SendVal | OpCode::SendVarEx)
                        && send.op1_type == inner_do.result_type
                        && send.op1 == inner_do.result
                        && send.op2 == 1
                        && outer_do.opcode == OpCode::DoFcall
                        && outer_do.result_type == OpType::Unused
                    {
                        Some((inner_init, inner_do, outer_do))
                    } else {
                        None
                    }
                } else {
                    None
                };

                has_object_call = true;
                if let Some((inner_init, _inner_do, _outer_do)) = nested {
                    add_mask_slot(
                        &mut object_input_mask,
                        inner_init.op1,
                        total_slots,
                    )?;
                    let resume_ip = ip;
                    let inner_guard = ScalarLongCallGuard::MethodCache {
                        cache_ip: u32::try_from(ip + 1).ok()?,
                        receiver_slot: inner_init.op1,
                    };
                    ip += 5;
                    QuickLongOp::ComposedPropertyCall {
                        outer_guard,
                        inner_guard,
                        next_target: QuickLongTarget::unresolved(ip)?,
                        resume_ip,
                    }
                } else {
                    let argument_count = usize::try_from(instruction.extended_value).ok()?;
                    if argument_count > 8 {
                        return None;
                    }
                    let mut arguments = [QuickLongOperand::Const(0); 8];
                    let mut cursor = ip + 1;
                    for (argument_index, argument) in arguments
                        .iter_mut()
                        .enumerate()
                        .take(argument_count)
                    {
                        let send = *op_array.instructions.get(cursor)?;
                        if !matches!(send.opcode, OpCode::SendVal | OpCode::SendVarEx)
                            || send.op2 as usize != argument_index + 1
                        {
                            return None;
                        }
                        *argument = match send.op1_type {
                            OpType::Cv | OpType::Tmp | OpType::Var => {
                                add_mask_slot(
                                    &mut long_input_mask,
                                    send.op1,
                                    total_slots,
                                )?;
                                QuickLongOperand::Slot(send.op1)
                            }
                            OpType::Const => QuickLongOperand::Const(long_literal(
                                op_array,
                                send.op1,
                            )?),
                            OpType::Unused => return None,
                        };
                        cursor += 1;
                    }
                    let do_fcall = *op_array.instructions.get(cursor)?;
                    if do_fcall.opcode != OpCode::DoFcall {
                        return None;
                    }
                    let resume_ip = ip;
                    ip = cursor + 1;
                    if do_fcall.result_type == OpType::Unused {
                        QuickLongOp::PropertyMethodCall {
                            guard: outer_guard,
                            arguments,
                            argument_count: argument_count as u8,
                            next_target: QuickLongTarget::unresolved(ip)?,
                            resume_ip,
                        }
                    } else if argument_count == 0
                        && matches!(do_fcall.result_type, OpType::Tmp | OpType::Var)
                    {
                        add_mask_slot(
                            &mut long_output_mask,
                            do_fcall.result,
                            total_slots,
                        )?;
                        QuickLongOp::PropertyGetterCall {
                            guard: outer_guard,
                            result: do_fcall.result,
                            next_target: QuickLongTarget::unresolved(ip)?,
                            resume_ip,
                        }
                    } else {
                        return None;
                    }
                }
            }
            OpCode::AssignCv => {
                if instruction.op1_type != OpType::Cv || instruction.result_type != OpType::Unused {
                    return None;
                }
                if string_key_assignment_mask & (1u64 << instruction.op1) != 0 {
                    add_mask_slot(&mut string_output_mask, instruction.op1, total_slots)?;
                    has_assign = true;
                    ip += 1;
                    match instruction.op2_type {
                        OpType::Const => QuickLongOp::AssignStringLiteral {
                            destination: instruction.op1,
                            literal: instruction.op2,
                            next_target: QuickLongTarget::unresolved(ip)?,
                        },
                        OpType::Cv => {
                            add_mask_slot(&mut string_input_mask, instruction.op2, total_slots)?;
                            QuickLongOp::AssignStringSlot {
                                destination: instruction.op1,
                                source: instruction.op2,
                                next_target: QuickLongTarget::unresolved(ip)?,
                            }
                        }
                        _ => return None,
                    }
                } else {
                    let source = long_slot(instruction.op2_type, instruction.op2)?;
                    add_mask_slot(&mut long_input_mask, source, total_slots)?;
                    add_mask_slot(&mut long_output_mask, instruction.op1, total_slots)?;
                    has_assign = true;
                    ip += 1;
                    QuickLongOp::Assign {
                        destination: instruction.op1,
                        source,
                        next_target: QuickLongTarget::unresolved(ip)?,
                    }
                }
            }
            OpCode::PostInc => {
                if instruction.op1_type != OpType::Cv {
                    return None;
                }
                let result = match instruction.result_type {
                    OpType::Unused => None,
                    OpType::Tmp => Some(instruction.result),
                    _ => return None,
                };
                add_mask_slot(&mut long_input_mask, instruction.op1, total_slots)?;
                add_mask_slot(&mut long_output_mask, instruction.op1, total_slots)?;
                if let Some(result) = result {
                    add_mask_slot(&mut long_output_mask, result, total_slots)?;
                }
                has_post_inc = true;
                let resume_ip = ip;
                if ip + 1 == backedge_ip {
                    let jump = op_array.instructions[backedge_ip];
                    if !matches!(jump.opcode, OpCode::Jmp | OpCode::QuickLongLoopJmp)
                        || jump.op1 as usize != header_ip
                    {
                        return None;
                    }
                    ip += 2;
                    QuickLongOp::PostIncJump {
                        value: instruction.op1,
                        result,
                        target: QuickLongTarget::unresolved(header_ip)?,
                        resume_ip,
                    }
                } else {
                    ip += 1;
                    QuickLongOp::PostInc {
                        value: instruction.op1,
                        result,
                        next_target: QuickLongTarget::unresolved(ip)?,
                        resume_ip,
                    }
                }
            }
            OpCode::Jmp | OpCode::QuickLongLoopJmp => {
                let target_ip = instruction.op1 as usize;
                if ip == backedge_ip {
                    if target_ip != header_ip {
                        return None;
                    }
                } else if target_ip <= ip {
                    return None;
                }
                ip += 1;
                QuickLongOp::Jump {
                    target: QuickLongTarget::unresolved(target_ip)?,
                }
            }
            _ => return None,
        };

        let op_ip = match op {
            QuickLongOp::BranchUnlessLt { resume_ip, .. }
            | QuickLongOp::BranchUnlessEq { resume_ip, .. }
            | QuickLongOp::ModConst { resume_ip, .. }
            | QuickLongOp::FetchArrayLong { resume_ip, .. }
            | QuickLongOp::Add { resume_ip, .. }
            | QuickLongOp::PropertyMethodCall { resume_ip, .. }
            | QuickLongOp::PropertyGetterCall { resume_ip, .. }
            | QuickLongOp::ComposedPropertyCall { resume_ip, .. }
            | QuickLongOp::PostInc { resume_ip, .. }
            | QuickLongOp::PostIncJump { resume_ip, .. }
            | QuickLongOp::PostIncLoopLt { resume_ip, .. } => resume_ip,
            QuickLongOp::AddAssign { add_resume_ip, .. } => add_resume_ip,
            QuickLongOp::ConditionalAddAssign {
                condition_resume_ip,
                ..
            } => condition_resume_ip,
            QuickLongOp::AddAddAssign {
                first_resume_ip, ..
            } => first_resume_ip,
            QuickLongOp::Assign { .. } => ip - 1,
            QuickLongOp::AssignStringLiteral { .. } => ip - 1,
            QuickLongOp::AssignStringSlot { .. } => ip - 1,
            QuickLongOp::Jump { .. } => ip - 1,
        };
        let relative = op_ip - header_ip;
        if ip_to_op[relative] != u16::MAX || ops.len() >= u16::MAX as usize {
            return None;
        }
        ip_to_op[relative] = ops.len() as u16;
        op_ips.push(u32::try_from(op_ip).ok()?);
        ops.push(op);
    }

    if let (
        Some(QuickLongOp::BranchUnlessLt {
            lhs,
            rhs,
            condition_tmp,
            false_target,
            next_target,
            ..
        }),
        Some(QuickLongOp::PostIncJump {
            value,
            result,
            target,
            resume_ip,
        }),
    ) = (ops.first().copied(), ops.last().copied())
    {
        if target.unresolved_ip()? == header_ip {
            *ops.last_mut()? = QuickLongOp::PostIncLoopLt {
                value,
                result,
                condition_lhs: lhs,
                condition_rhs: rhs,
                condition_tmp,
                body_target: next_target,
                exit_target: false_target,
                resume_ip,
            };
        }
    }

    let has_internal_branch = ops.iter().skip(1).any(|op| {
        matches!(
            op,
            QuickLongOp::BranchUnlessLt { .. } | QuickLongOp::BranchUnlessEq { .. }
        )
    });
    if !(has_add || has_assign || has_internal_branch || has_object_call)
        || !has_post_inc
        || !matches!(
            ops.first(),
            Some(QuickLongOp::BranchUnlessLt { false_target, .. })
                if matches!(false_target.unresolved_ip(), Some(ip) if ip > backedge_ip)
        )
        || !(matches!(
            ops.last(),
            Some(QuickLongOp::Jump { target } | QuickLongOp::PostIncJump { target, .. })
                if target.unresolved_ip() == Some(header_ip)
        ) || matches!(ops.last(), Some(QuickLongOp::PostIncLoopLt { .. })))
    {
        return None;
    }

    let entry_op = ip_to_op.first().copied()?;
    if entry_op == u16::MAX {
        return None;
    }
    for op in &mut ops {
        op.resolve_targets(
            header_ip,
            backedge_ip,
            op_array.instructions.len(),
            &ip_to_op,
        )?;
    }

    let long_mask = long_input_mask | long_output_mask;
    if long_mask & bool_output_mask != 0
        || array_input_mask & (long_mask | bool_output_mask) != 0
        || string_input_mask & (long_mask | bool_output_mask | array_input_mask) != 0
        || string_output_mask & !string_input_mask != 0
        || object_input_mask
            & (long_mask | bool_output_mask | array_input_mask | string_input_mask)
            != 0
    {
        return None;
    }
    let involved_mask = long_input_mask
        | long_output_mask
        | bool_output_mask
        | array_input_mask
        | string_input_mask
        | string_output_mask
        | object_input_mask;

    Some(QuickLongOpsLoop {
        header_ip,
        backedge_ip,
        ops,
        entry_op,
        op_ips,
        long_input_mask,
        long_output_mask,
        bool_output_mask,
        array_input_mask,
        string_input_mask,
        string_output_mask,
        object_input_mask,
        string_cache_capacity: string_cache_capacity as u8,
        involved_mask,
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::compiler::compile::Compiler;
    use crate::compiler::make_user_function;
    use crate::lexer::Lexer;
    use crate::parser::Parser;

    fn compile_main(source: &str) -> crate::vm::function::UserFunction {
        let tokens = Lexer::new(source).tokenize().unwrap();
        let statements = Parser::new(tokens).parse().unwrap();
        let result = Compiler::new().compile(&statements).unwrap();
        make_user_function(result.main)
    }

    fn quick_plan(source: &str) -> QuickLongAccumulateLoop {
        let main = compile_main(source);
        let selected_backedge = main
            .op_array
            .instructions
            .iter()
            .position(|instruction| instruction.opcode == OpCode::QuickLongLoopJmp);
        #[cfg(feature = "quick-loops")]
        assert!(
            selected_backedge.is_some(),
            "compiler should select a quick loop"
        );
        let backedge = selected_backedge
            .or_else(|| {
                main.op_array
                    .instructions
                    .iter()
                    .enumerate()
                    .position(|(ip, instruction)| {
                        instruction.opcode == OpCode::Jmp && (instruction.op1 as usize) < ip
                    })
            })
            .expect("source should contain a backward edge");
        let header = main.op_array.instructions[backedge].op1 as usize;
        detect_long_accumulate_loop(&main.op_array, header, backedge).unwrap()
    }

    fn induction_plan(source: &str) -> QuickLongInductionLoop {
        let main = compile_main(source);
        main.op_array
            .instructions
            .iter()
            .enumerate()
            .filter(|(ip, instruction)| {
                matches!(instruction.opcode, OpCode::Jmp | OpCode::QuickLongLoopJmp)
                    && (instruction.op1 as usize) < *ip
            })
            .find_map(|(backedge, instruction)| {
                detect_long_induction_loop(
                    &main.op_array,
                    instruction.op1 as usize,
                    backedge,
                )
            })
            .expect("source should contain an induction-only quick loop")
    }

    fn long_ops_plan(source: &str) -> QuickLongOpsLoop {
        let main = compile_main(source);
        main.op_array
            .instructions
            .iter()
            .enumerate()
            .filter(|(ip, instruction)| {
                matches!(instruction.opcode, OpCode::Jmp | OpCode::QuickLongLoopJmp)
                    && (instruction.op1 as usize) < *ip
            })
            .find_map(|(backedge, instruction)| {
                detect_long_ops_loop(&main.op_array, instruction.op1 as usize, backedge)
            })
            .unwrap_or_else(|| {
                panic!(
                    "source should contain a typed long ops loop; instructions: {:#?}",
                    main.op_array.instructions
                )
            })
    }

    #[test]
    fn detects_guarded_property_calls_inside_general_long_ops_loop() {
        let plan = long_ops_plan(
            "<?php
class Tick {
    public $value = 0;
    public function advance() { $this->value = $this->value + 1; }
    public function current() { return $this->value; }
}
class Sink {
    public $value = 0;
    public function accept($value) { $this->value = $this->value + $value; }
}
$tick = new Tick();
$sink = new Sink();
for ($i = 0; $i < 100; $i++) {
    $tick->advance();
    if ($i % 3 == 0) {
        $sink->accept($tick->current());
    }
}
",
        );
        assert_eq!(plan.object_input_mask, (1u64 << 0) | (1u64 << 1));
        assert!(plan.ops.iter().any(|operation| matches!(
            operation,
            QuickLongOp::PropertyMethodCall {
                argument_count: 0,
                ..
            }
        )));
        assert!(plan.ops.iter().any(|operation| matches!(
            operation,
            QuickLongOp::ComposedPropertyCall { .. }
        )));
    }

    fn foreach_long_accumulate_plan(source: &str) -> QuickForeachLongAccumulateLoop {
        let main = compile_main(source);
        main.op_array
            .instructions
            .iter()
            .enumerate()
            .filter(|(ip, instruction)| {
                matches!(instruction.opcode, OpCode::Jmp | OpCode::QuickLongLoopJmp)
                    && (instruction.op1 as usize) < *ip
            })
            .find_map(|(backedge, instruction)| {
                detect_foreach_long_accumulate_loop(
                    &main.op_array,
                    instruction.op1 as usize,
                    backedge,
                )
            })
            .unwrap_or_else(|| {
                panic!(
                    "source should contain a foreach long accumulation loop; instructions: {:#?}",
                    main.op_array.instructions
                )
            })
    }

    #[test]
    fn detects_value_only_foreach_long_accumulation() {
        let source = "<?php
$values = [1, 2, 3, 4];
$sum = 0;
foreach ($values as $value) {
    $sum += $value;
}
";
        let plan = foreach_long_accumulate_plan(source);
        assert_eq!(plan.accumulator_cv, 1);
        assert_eq!(plan.value_cv, 2);
        assert_eq!(plan.sum_ip, plan.header_ip + 2);
        assert_eq!(plan.exit_ip, plan.header_ip + 5);

        #[cfg(feature = "quick-loops")]
        {
            let main = compile_main(source);
            assert!(main.op_array.block_plans.iter().any(|plan| matches!(
                plan,
                crate::vm::planner::BlockPlan::QuickForeachLongAccumulate(_)
            )));
        }
    }

    #[test]
    fn rejects_key_value_foreach_long_accumulation() {
        let main = compile_main(
            "<?php
$values = [1, 2, 3, 4];
$sum = 0;
foreach ($values as $key => $value) {
    $sum += $value;
}
",
        );
        assert!(main
            .op_array
            .instructions
            .iter()
            .enumerate()
            .filter(|(ip, instruction)| {
                matches!(instruction.opcode, OpCode::Jmp | OpCode::QuickLongLoopJmp)
                    && (instruction.op1 as usize) < *ip
            })
            .all(|(backedge, instruction)| {
                detect_foreach_long_accumulate_loop(
                    &main.op_array,
                    instruction.op1 as usize,
                    backedge,
                )
                .is_none()
            }));
    }

    #[test]
    fn detects_induction_plus_constant_with_cv_bound() {
        let plan = quick_plan(
            "<?php
$n = 100;
$sum = 0;
for ($i = 0; $i < $n; $i++) {
    $sum += $i + 1;
}
",
        );
        assert_eq!(plan.induction_cv, 2);
        assert_eq!(plan.accumulator_cv, 1);
        assert!(matches!(plan.bound, QuickLongBound::Cv(0)));
        assert!(matches!(
            plan.term,
            QuickLongTerm::InductionPlusConst { addend: 1, .. }
        ));
        assert_eq!(plan.exit_ip, 10);
    }

    #[test]
    fn detects_induction_plus_invariant_cv_in_either_order() {
        for expression in ["$i + $offset", "$offset + $i"] {
            let plan = quick_plan(&format!(
                "<?php
$offset = 7;
$sum = 0;
for ($i = 0; $i < 100; $i++) {{
    $sum += {expression};
}}
"
            ));
            assert_eq!(plan.induction_cv, 2);
            assert_eq!(plan.accumulator_cv, 1);
            assert!(matches!(
                plan.term,
                QuickLongTerm::InductionPlusCv { addend_cv: 0, .. }
            ));
        }
    }

    #[test]
    fn detects_direct_scalar_function_call_accumulation() {
        let plan = quick_plan(
            "<?php
function affine($value, $scale, $bias) {
    return $value * $scale + $bias;
}
$scale = 2;
$sum = 0;
for ($i = 0; $i < 100; $i++) {
    $sum += affine($i, $scale, 1);
}
",
        );
        assert_eq!(plan.induction_cv, 2);
        assert_eq!(plan.accumulator_cv, 1);
        assert!(matches!(
            plan.term,
            QuickLongTerm::ScalarFunctionCall {
                argument_count: 3,
                long_input_mask,
                guard,
                do_fcall_ip,
                ..
            } if long_input_mask == (1u64 << 0) | (1u64 << 2)
                && matches!(guard, ScalarLongCallGuard::FunctionCache { .. })
                && do_fcall_ip == guard.cache_ip() + 4
        ));
    }

    #[test]
    fn detects_scalar_expression_in_function_call_argument() {
        let plan = quick_plan(
            "<?php
function combine($left, $right) {
    return $left + $right;
}
$sum = 0;
for ($i = 0; $i < 100; $i++) {
    $sum += combine($i, $i + 1);
}
",
        );
        assert!(matches!(
            plan.term,
            QuickLongTerm::ScalarFunctionCall {
                argument_count: 2,
                long_input_mask,
                guard,
                do_fcall_ip,
                ..
            } if long_input_mask == 1u64 << 1
                && matches!(guard, ScalarLongCallGuard::FunctionCache { .. })
                && do_fcall_ip == guard.cache_ip() + 4
        ));
    }

    #[test]
    fn compiles_scalar_arguments_into_typed_plan() {
        let plan = quick_plan(
            "<?php
function combine($left, $right) {
    return $left + $right;
}
$sum = 0;
for ($i = 0; $i < 100; $i++) {
    $sum += combine($i, $i + 1);
}
",
        );
        let QuickLongTerm::ScalarFunctionCall { argument_plan, .. } = plan.term else {
            panic!("expected scalar function call term");
        };
        assert_eq!(argument_plan.operations.len(), 1);
        assert!(matches!(
            argument_plan.operations[0],
            ScalarLongOp {
                kind: ScalarLongOpKind::Add,
                lhs: ScalarLongSource::Input(1),
                rhs: ScalarLongSource::Constant(1),
            }
        ));
        assert_eq!(
            argument_plan.outputs[0],
            ScalarLongSource::Input(1)
        );
        assert_eq!(
            argument_plan.outputs[1],
            ScalarLongSource::Temporary(0)
        );
    }

    #[test]
    fn composes_argument_and_leaf_program_temporary_indices() {
        let mut argument_outputs = [ScalarLongSource::Constant(0); 8];
        argument_outputs[0] = ScalarLongSource::Input(1);
        argument_outputs[1] = ScalarLongSource::Temporary(0);
        let arguments = ScalarLongProgram {
            operations: vec![ScalarLongOp {
                kind: ScalarLongOpKind::Add,
                lhs: ScalarLongSource::Input(1),
                rhs: ScalarLongSource::Constant(1),
            }]
            .into_boxed_slice(),
            outputs: argument_outputs,
            output_count: 2,
        };
        let body = ScalarLongFunctionPlan {
            public_args: 2,
            program: ScalarLongProgram {
                operations: vec![
                    ScalarLongOp {
                        kind: ScalarLongOpKind::Multiply,
                        lhs: ScalarLongSource::Input(0),
                        rhs: ScalarLongSource::Input(1),
                    },
                    ScalarLongOp {
                        kind: ScalarLongOpKind::Add,
                        lhs: ScalarLongSource::Temporary(0),
                        rhs: ScalarLongSource::Constant(3),
                    },
                ]
                .into_boxed_slice(),
                outputs: [ScalarLongSource::Temporary(1)],
                output_count: 1,
            },
        };

        let fused = compose_quick_scalar_leaf_program(&arguments, &body).unwrap();
        assert_eq!(fused.operations.len(), 3);
        assert!(matches!(
            fused.operations[1],
            ScalarLongOp {
                kind: ScalarLongOpKind::Multiply,
                lhs: ScalarLongSource::Input(1),
                rhs: ScalarLongSource::Temporary(0),
            }
        ));
        assert!(matches!(
            fused.operations[2],
            ScalarLongOp {
                kind: ScalarLongOpKind::Add,
                lhs: ScalarLongSource::Temporary(1),
                rhs: ScalarLongSource::Constant(3),
            }
        ));
        assert_eq!(fused.outputs[0], ScalarLongSource::Temporary(2));
    }

    #[test]
    fn detects_nested_monomorphic_scalar_method_accumulation() {
        let plan = quick_plan(
            "<?php
class Math {
    public function add($left, $right) { return $left + $right; }
    public function mul($left, $right) { return $left * $right; }
}
$math = new Math();
$sum = 0;
for ($i = 0; $i < 100; $i++) {
    $sum += $math->add($i, $math->mul($i, 2));
}
",
        );
        assert!(matches!(
            plan.term,
            QuickLongTerm::ScalarMethodCall {
                argument_count: 2,
                long_input_mask,
                object_input_mask,
                guard,
                do_fcall_ip,
                ..
            } if long_input_mask == 1u64 << 2
                && object_input_mask == 1u64 << 0
                && matches!(guard, ScalarLongCallGuard::MethodCache {
                    receiver_slot: 0,
                    ..
                })
                && do_fcall_ip == guard.cache_ip() + 7
        ));
    }

    #[test]
    fn detects_invariant_string_length_as_accumulate_term() {
        for update in ["$i++", "++$i"] {
            let source = format!("<?php
$string = 'abcd';
$sum = 0;
for ($i = 0; $i < 100; {update}) {{
    $sum += strlen($string);
}}
");
            let plan = quick_plan(&source);
            assert_eq!(plan.induction_cv, 2);
            assert_eq!(plan.accumulator_cv, 1);
            assert!(matches!(
                plan.term,
                QuickLongTerm::StringLength { string_cv: 0, .. }
            ));
            assert_eq!(
                plan.increment_kind,
                if update == "++$i" {
                    QuickIncrementKind::Pre
                } else {
                    QuickIncrementKind::Post
                }
            );

            #[cfg(feature = "quick-loops")]
            {
                let main = compile_main(&source);
                assert!(main.op_array.block_plans.iter().any(|plan| matches!(
                    plan,
                    crate::vm::planner::BlockPlan::QuickLongAccumulate(_)
                )));
            }
        }
    }

    #[test]
    fn detects_long_abs_as_accumulate_term() {
        for expression in ["abs($value)", "abs($i)"] {
            let source = format!("<?php
$value = -7;
$sum = 0;
for ($i = 0; $i < 100; ++$i) {{
    $sum += {expression};
}}
");
            let plan = quick_plan(&source);
            assert!(matches!(
                plan.term,
                QuickLongTerm::AbsLong { operand_cv, .. }
                    if operand_cv == if expression == "abs($i)" { 2 } else { 0 }
            ));
        }
    }

    #[test]
    fn detects_prefix_and_postfix_induction_only_loops() {
        let postfix = induction_plan("<?php
$limit = 100;
$i = 0;
while ($i < $limit) {
    $i++;
}
");
        assert!(matches!(postfix.bound, QuickLongBound::Cv(0)));
        assert_eq!(postfix.increment_kind, QuickIncrementKind::Post);

        let prefix = induction_plan("<?php
for ($i = 0; $i < 100; ++$i) {
}
");
        assert!(matches!(prefix.bound, QuickLongBound::Const(100)));
        assert_eq!(prefix.increment_kind, QuickIncrementKind::Pre);

        #[cfg(feature = "quick-loops")]
        {
            let main = compile_main("<?php
for ($i = 0; $i < 100; ++$i) {
}
");
            assert!(main.op_array.block_plans.iter().any(|plan| matches!(
                plan,
                crate::vm::planner::BlockPlan::QuickLongInduction(_)
            )));
        }
    }

    #[test]
    fn detects_branch_only_if_else_loop_as_typed_ops() {
        let source = "<?php
for ($i = 0; $i < 100; $i++) {
    if ($i == -1) {
    } elseif ($i == -2) {
    } else if ($i == -3) {
    }
}
";
        let plan = long_ops_plan(source);
        assert_eq!(
            plan.ops
                .iter()
                .filter(|op| matches!(op, QuickLongOp::BranchUnlessEq { .. }))
                .count(),
            3
        );
        assert!(matches!(
            plan.ops.last(),
            Some(QuickLongOp::PostIncLoopLt { .. })
        ));

        #[cfg(feature = "quick-loops")]
        {
            let main = compile_main(source);
            assert!(main.op_array.block_plans.iter().any(|plan| matches!(
                plan,
                crate::vm::planner::BlockPlan::QuickLongOps(_)
            )));
        }
    }

    #[test]
    fn detects_packed_array_index_as_accumulate_term() {
        let plan = quick_plan(
            "<?php
$values = [1, 2, 3, 4];
$sum = 0;
for ($i = 0; $i < 4; $i++) {
    $sum += $values[$i];
}
",
        );
        assert!(matches!(
            plan.term,
            QuickLongTerm::ArrayIndex {
                array_cv: 0,
                index: QuickArrayIndex::Long(QuickLongOperand::Slot(2)),
                ..
            }
        ));
    }

    #[test]
    fn detects_string_literal_array_index_as_accumulate_term() {
        let plan = quick_plan(
            "<?php
$values = ['hot' => 7];
$sum = 0;
for ($i = 0; $i < 100; $i++) {
    $sum += $values['hot'];
}
",
        );
        assert!(matches!(
            plan.term,
            QuickLongTerm::ArrayIndex {
                array_cv: 0,
                index: QuickArrayIndex::StringLiteral(_),
                ..
            }
        ));
    }

    #[test]
    fn detects_invariant_value_slot_array_index_as_accumulate_term() {
        for body in [
            "$sum += $values[$key];",
            "$value = $values[$key];\n    $sum += $value;",
        ] {
            let plan = quick_plan(&format!(
                "<?php
$values = ['hot' => 7];
$key = 'hot';
$sum = 0;
$value = 0;
for ($i = 0; $i < 100; $i++) {{
    {body}
}}
"
            ));
            assert!(matches!(
                plan.term,
                QuickLongTerm::ArrayIndex {
                    index: QuickArrayIndex::ValueSlot(1),
                    ..
                }
            ));
        }
    }

    #[test]
    fn detects_materialized_invariant_array_index_as_accumulate_term() {
        for index in ["'hot'", "7"] {
            let plan = quick_plan(&format!(
                "<?php
$values = ['hot' => 7, 7 => 9];
$sum = 0;
$value = 0;
for ($i = 0; $i < 100; $i++) {{
    $value = $values[{index}];
    $sum += $value;
}}
"
            ));
            assert!(matches!(
                plan.term,
                QuickLongTerm::ArrayIndex {
                    array_cv: 0,
                    index:
                        QuickArrayIndex::StringLiteral(_)
                        | QuickArrayIndex::Long(QuickLongOperand::Const(7)),
                    destination: Some(2),
                    ..
                }
            ));
        }
    }

    #[test]
    fn detects_direct_accumulation_with_constant_bound() {
        let plan = quick_plan(
            "<?php
$sum = 0;
for ($i = 0; $i < 100; $i++) {
    $sum += $i;
}
",
        );
        assert_eq!(plan.induction_cv, 1);
        assert_eq!(plan.accumulator_cv, 0);
        assert!(matches!(plan.bound, QuickLongBound::Const(100)));
        assert!(matches!(plan.term, QuickLongTerm::Induction));
        assert_eq!(plan.condition_tmp, None);
    }

    #[test]
    fn detects_two_cv_nested_term_as_typed_ops() {
        let plan = long_ops_plan(
            "<?php
$sum = 0;
for ($i = 0; $i < 10; $i++) {
    for ($j = 0; $j < 20; $j++) {
        $sum += $i + $j;
    }
}
",
        );
        assert_eq!(plan.ops.len(), 3);
        assert_eq!(
            plan.ops
                .iter()
                .filter(|op| matches!(op, QuickLongOp::AddAddAssign { .. }))
                .count(),
            1
        );
        assert!(matches!(
            plan.ops.first(),
            Some(QuickLongOp::BranchUnlessLt {
                rhs: QuickLongOperand::Const(20),
                ..
            })
        ));
    }

    #[test]
    fn detects_materialized_array_long_read_as_selected_typed_ops() {
        let source = "<?php
$values = [1, 2, 3, 4];
$sum = 0;
for ($i = 0; $i < 4; $i++) {
    $value = $values[$i];
    $sum += $value;
}
";
        let plan = long_ops_plan(source);
        assert!(plan
            .ops
            .iter()
            .any(|op| matches!(
                op,
                QuickLongOp::FetchArrayLong {
                    destination: Some(_),
                    ..
                }
            )));
        assert!(matches!(
            plan.ops.as_slice(),
            [
                QuickLongOp::BranchUnlessLt { .. },
                QuickLongOp::FetchArrayLong { .. },
                QuickLongOp::AddAssign { .. },
                QuickLongOp::PostIncLoopLt { .. },
            ]
        ));
        #[cfg(feature = "quick-loops")]
        {
            let main = compile_main(source);
            assert!(main.op_array.block_plans.iter().any(|plan| matches!(
                plan,
                crate::vm::planner::BlockPlan::QuickLongOps(_)
            )));
        }
        assert_ne!(plan.array_input_mask, 0);
        assert_eq!(
            plan.array_input_mask
                & (plan.long_input_mask | plan.long_output_mask | plan.bool_output_mask),
            0
        );
    }

    #[test]
    fn detects_string_literal_hash_read_as_typed_op() {
        let plan = long_ops_plan(
            "<?php
$values = ['hot' => 7];
$sum = 0;
$last = 0;
for ($i = 0; $i < 100; $i++) {
    $last = $values['hot'];
    $sum += $i;
}
",
        );
        assert!(plan.ops.iter().any(|op| matches!(
            op,
            QuickLongOp::FetchArrayLong {
                index: QuickArrayIndex::StringLiteral(_),
                ..
            }
        )));
        assert_ne!(plan.array_input_mask, 0);
    }

    #[test]
    fn detects_strided_integer_hash_scan_as_typed_ops() {
        let plan = long_ops_plan(
            "<?php
$values = [100 => 3, 107 => 5, 114 => 7];
$stride = 7;
$key = 100;
$sum = 0;
for ($i = 0; $i < 3; $i++) {
    $sum += $values[$key];
    $key = $key + $stride;
}
",
        );
        assert_eq!(plan.ops.len(), 5, "{:#?}", plan.ops);
        assert!(matches!(
            plan.ops.as_slice(),
            [
                QuickLongOp::BranchUnlessLt { .. },
                QuickLongOp::FetchArrayLong {
                    index: QuickArrayIndex::Long(QuickLongOperand::Slot(_)),
                    ..
                },
                QuickLongOp::AddAssign { .. },
                QuickLongOp::AddAssign { .. },
                QuickLongOp::PostIncLoopLt { .. },
            ]
        ));
    }

    #[test]
    fn detects_materialized_hash_value_with_two_aggregates_as_typed_ops() {
        let plan = long_ops_plan(
            "<?php
$values = [100 => 3, 107 => 5, 114 => 7];
$key = 100;
$sum = 0;
$adjusted = 0;
$one = 1;
$stride = 7;
for ($i = 0; $i < 3; $i++) {
    $value = $values[$key];
    $sum += $value;
    $adjusted += $value + $one;
    $key = $key + $stride;
}
",
        );
        assert_eq!(
            plan.ops
                .iter()
                .filter(|op| matches!(op, QuickLongOp::FetchArrayLong { .. }))
                .count(),
            1
        );
        assert!(plan
            .ops
            .iter()
            .any(|op| matches!(
                op,
                QuickLongOp::FetchArrayLong {
                    destination: Some(_),
                    ..
                }
            )));
    }

    #[test]
    fn detects_filtered_hash_aggregate_as_typed_ops() {
        let plan = long_ops_plan(
            "<?php
$values = [100 => 3, 107 => 5, 114 => 7];
$key = 100;
$sum = 0;
$stride = 7;
for ($i = 0; $i < 3; $i++) {
    $value = $values[$key];
    if ($value < 6) {
        $sum += $value;
    }
    $key = $key + $stride;
}
",
        );
        assert_eq!(
            plan.ops
                .iter()
                .filter(|op| matches!(op, QuickLongOp::FetchArrayLong { .. }))
                .count(),
            1
        );
        assert!(plan
            .ops
            .iter()
            .any(|op| matches!(op, QuickLongOp::BranchUnlessLt { .. })));
        assert!(plan.ops.iter().any(|op| matches!(
            op,
            QuickLongOp::FetchArrayLong {
                destination: Some(_),
                ..
            }
        )));
        assert!(plan
            .ops
            .iter()
            .any(|op| matches!(op, QuickLongOp::ConditionalAddAssign { .. })));
    }

    #[test]
    fn detects_conditional_body_as_internal_branch() {
        let plan = long_ops_plan(
            "<?php
$n = 100;
$cutoff = 50;
$sum = 0;
for ($i = 0; $i < $n; $i++) {
    if ($i < $cutoff) {
        $sum += $i;
    }
}
",
        );
        assert_eq!(
            plan.ops
                .iter()
                .filter(|op| matches!(op, QuickLongOp::BranchUnlessLt { .. }))
                .count(),
            1
        );
        assert_eq!(plan.entry_op, 0);
        match plan.ops.first() {
            Some(QuickLongOp::BranchUnlessLt {
                false_target,
                next_target,
                ..
            }) => {
                assert!(false_target.exit_ip().is_some());
                assert!(next_target.op_index().is_some());
            }
            op => panic!("expected an entry less-than branch, got {op:?}"),
        }
        assert!(plan.ops.iter().any(|op| matches!(
            op,
            QuickLongOp::ConditionalAddAssign {
                condition: QuickLongCondition::Lt { .. },
                ..
            }
        )));
        assert!(matches!(
            plan.ops.last(),
            Some(QuickLongOp::PostIncLoopLt {
                body_target,
                exit_target,
                ..
            }) if body_target.op_index().is_some() && exit_target.exit_ip().is_some()
        ));
    }

    #[test]
    fn detects_modulo_equality_conditional_body() {
        let plan = long_ops_plan(
            "<?php
$n = 100;
$sum = 0;
for ($i = 0; $i < $n; $i++) {
    if (($i % 2) == 0) {
        $sum += $i;
    }
}
",
        );
        assert!(plan
            .ops
            .iter()
            .any(|op| matches!(op, QuickLongOp::ModConst { divisor: 2, .. })));
        assert!(plan.ops.iter().any(|op| matches!(
            op,
            QuickLongOp::ConditionalAddAssign {
                condition: QuickLongCondition::Eq {
                    rhs: QuickLongOperand::Const(0),
                    ..
                },
                ..
            }
        )));
    }

    #[test]
    fn detects_dynamic_string_array_key_state() {
        let plan = long_ops_plan(
            "<?php
$values = ['left' => 3, 'right' => 5];
$key = 'left';
$sum = 0;
for ($i = 0; $i < 100; $i++) {
    $sum += $values[$key];
    if (($i % 2) == 0) {
        $key = 'right';
    } else {
        $key = 'left';
    }
}
",
        );
        assert!(plan.string_input_mask != 0);
        assert_eq!(plan.string_input_mask, plan.string_output_mask);
        assert_eq!(plan.string_cache_capacity, 2);
        assert!(plan.ops.iter().any(|op| matches!(
            op,
            QuickLongOp::FetchArrayLong {
                index: QuickArrayIndex::ValueSlot(_),
                ..
            }
        )));
        assert_eq!(
            plan.ops
                .iter()
                .filter(|op| matches!(op, QuickLongOp::AssignStringLiteral { .. }))
                .count(),
            2
        );
    }

    #[test]
    fn sizes_dynamic_string_cache_from_distinct_loop_literals() {
        let plan = long_ops_plan(
            "<?php
$values = ['left' => 3, 'right' => 5, 'middle' => 7];
$key = 'left';
$sum = 0;
for ($i = 0; $i < 100; $i++) {
    $sum += $values[$key];
    $remainder = $i % 3;
    if ($remainder == 0) {
        $key = 'right';
    } else {
        if ($remainder == 1) {
            $key = 'middle';
        } else {
            $key = 'left';
        }
    }
}
",
        );
        assert_eq!(plan.string_cache_capacity, 3);
    }

    #[test]
    fn detects_dynamic_string_array_key_sources() {
        let plan = long_ops_plan(
            "<?php
$values = ['left' => 3, 'right' => 5];
$left = 'left';
$right = 'right';
$key = $left;
$sum = 0;
for ($i = 0; $i < 100; $i++) {
    $sum += $values[$key];
    if (($i % 2) == 0) {
        $key = $right;
    } else {
        $key = $left;
    }
}
",
        );
        assert_eq!(plan.string_input_mask.count_ones(), 3);
        assert_eq!(plan.string_output_mask.count_ones(), 1);
        assert_eq!(plan.string_cache_capacity, 2);
        assert_eq!(
            plan.ops
                .iter()
                .filter(|op| matches!(op, QuickLongOp::AssignStringSlot { .. }))
                .count(),
            2
        );
    }

    #[test]
    fn keeps_dynamic_integer_key_sources_on_long_state() {
        let plan = long_ops_plan(
            "<?php
$values = [100 => 3, 107 => 5];
$left = 100;
$right = 107;
$key = $left;
$sum = 0;
for ($i = 0; $i < 100; $i++) {
    $sum += $values[$key];
    if (($i % 2) == 0) {
        $key = $right;
    } else {
        $key = $left;
    }
}
",
        );
        assert_eq!(plan.string_input_mask, 0);
        assert_eq!(plan.string_output_mask, 0);
        assert_eq!(
            plan.ops
                .iter()
                .filter(|op| matches!(op, QuickLongOp::Assign { .. }))
                .count(),
            2
        );
    }
}
