//! Quickened, guarded execution regions.
//!
//! This module deliberately starts with one family of closed scalar loops. The
//! baseline bytecode remains the semantic source of truth; detection only
//! creates a compact description that `execute_ex` may run after the backedge
//! becomes hot.

use crate::compiler::OpArray;
use crate::value::Value;
use crate::vm::instruction::OpType;
use crate::vm::opcode::OpCode;

/// Number of executions of the same backward edge before quickening it.
pub const QUICK_LOOP_HOT_THRESHOLD: u32 = 32;
/// Consecutive failed activations before a region stays in baseline.
pub const QUICK_LOOP_FAILURE_LIMIT: u32 = 3;
/// Counter states reserve one full hotness interval per failed activation.
pub const QUICK_LOOP_COUNTER_STRIDE: u32 = QUICK_LOOP_HOT_THRESHOLD + 1;
pub const QUICK_LOOP_DISABLED: u32 = u32::MAX;

#[derive(Debug, Clone, Copy)]
pub enum QuickLongBound {
    Cv(u16),
    Const(i64),
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum QuickArrayIndex {
    Long(QuickLongOperand),
    StringLiteral(u16),
}

#[derive(Debug, Clone, Copy)]
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
}

/// Region for the compiler shapes produced by:
///
/// ```php
/// for (...; $i < $limit; $i++) {
///     $accumulator += $i;
///     // or: $accumulator += $i + INTEGER_CONSTANT;
///     // or: $accumulator += $i + $loop_invariant_cv;
///     // or: $accumulator += $packed_array[$i];
///     // or: $value = $array['key']; $accumulator += $value;
/// }
/// ```
///
/// The baseline region contains comparison, conditional exit, accumulation,
/// assignment, post-increment and backward jump, with an optional arithmetic
/// term instruction. All observable state is scalar and every deoptimization
/// point has a precise baseline instruction.
#[derive(Debug, Clone, Copy)]
pub struct QuickLongAccumulateLoop {
    pub header_ip: usize,
    pub exit_ip: usize,
    pub induction_cv: u16,
    pub bound: QuickLongBound,
    pub accumulator_cv: u16,
    pub condition_tmp: Option<u16>,
    pub term: QuickLongTerm,
    pub sum_tmp: u16,
    pub post_tmp: Option<u16>,
    pub sum_ip: usize,
    pub post_inc_ip: usize,
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
    Assign {
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
            | Self::Assign { next_target, .. }
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

/// A prevalidated typed program for a closed long-only scalar loop.
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
        || header_ip.checked_add(7)? < backedge_ip
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
    let (accumulator_cv, term, sum_tmp, sum_ip, assign_ip) = if backedge_ip == header_ip + 5 {
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
            (OpCode::FetchDimR, OpType::Cv, OpType::Cv) if first_body.op2 == induction_cv => {
                QuickLongTerm::ArrayIndex {
                    array_cv: first_body.op1,
                    index: QuickArrayIndex::Long(QuickLongOperand::Slot(induction_cv)),
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
            _ => return None,
        };
        (sum.op1, term, sum.result, header_ip + 3, header_ip + 4)
    } else {
        let materialize = op_array.instructions[header_ip + 3];
        let sum = op_array.instructions[header_ip + 4];
        if first_body.opcode != OpCode::FetchDimR
            || first_body.op1_type != OpType::Cv
            || first_body.op2_type != OpType::Const
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
                index: array_literal_index(op_array, first_body.op2)?,
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

    let post_inc_ip = assign_ip + 1;
    let post_inc = op_array.instructions[post_inc_ip];
    if post_inc.opcode != OpCode::PostInc
        || post_inc.op1_type != OpType::Cv
        || post_inc.op1 != induction_cv
    {
        return None;
    }
    let post_tmp = match post_inc.result_type {
        OpType::Unused => None,
        OpType::Tmp => Some(post_inc.result),
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
        | QuickLongTerm::ArrayIndex { term_tmp, .. } => {
            temporary_slots.push(term_tmp);
        }
    }
    if let Some(slot) = post_tmp {
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
        post_tmp,
        sum_ip,
        post_inc_ip,
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
    let mut has_add = false;
    let mut has_assign = false;
    let mut has_post_inc = false;
    let mut ip = header_ip;

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
                        add_mask_slot(&mut long_input_mask, instruction.op2, total_slots)?;
                        QuickArrayIndex::Long(QuickLongOperand::Slot(instruction.op2))
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
            OpCode::AssignCv => {
                if instruction.op1_type != OpType::Cv || instruction.result_type != OpType::Unused {
                    return None;
                }
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

    if !has_add
        || !has_assign
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
    {
        return None;
    }
    let involved_mask =
        long_input_mask | long_output_mask | bool_output_mask | array_input_mask;

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
}
