//! Quickened, guarded execution regions.
//!
//! This module deliberately starts with one family of closed scalar loops. The
//! baseline bytecode remains the semantic source of truth; detection only
//! creates a compact description that `execute_ex` may run after the backedge
//! becomes hot.

use crate::compiler::OpArray;
use crate::value::Value;
use crate::vm::function::{
    ScalarLongCallGuard, ScalarLongConditionKind, ScalarLongFunctionPlan, ScalarLongOp,
    ScalarLongOpKind, ScalarLongProgram, ScalarLongSource,
};
use crate::vm::instruction::OpType;
use crate::vm::opcode::OpCode;

pub use super::quick_foreach_plan::{
    QuickForeachObjectProjection, QuickForeachObjectProjectionKind,
    QuickForeachObjectPropertyAccumulateLoop,
    detect_foreach_object_property_accumulate_loop,
};

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

/// A straight-line `fetch -> scalar arithmetic -> replace existing entry`
/// sequence attached to its fetch operation. Other control-flow entries keep
/// the original operations, while the common path performs one lookup and one
/// quick-op dispatch.
#[derive(Debug, Clone, Copy)]
pub struct QuickArrayUpdateFusion {
    pub kind: ScalarLongOpKind,
    pub lhs: QuickLongOperand,
    pub rhs: QuickLongOperand,
    pub result: u16,
    pub next_target: QuickLongTarget,
    pub arithmetic_resume_ip: usize,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum QuickStringAppendSource {
    Literal(u16),
    Slot(u16),
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum QuickVirtualValueSource {
    Long(QuickLongOperand),
    StringLiteral(u16),
    StringSlot(u16),
}

#[derive(Debug, Clone, Copy)]
pub struct QuickObjectArrayConsumer {
    pub key_literal: u16,
    pub accumulator: u16,
}

impl QuickObjectArrayConsumer {
    pub const EMPTY: Self = Self {
        key_literal: 0,
        accumulator: 0,
    };
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
    /// A nested scalar function/method call tree. Object receivers, when
    /// present, are invariant CVs. Runtime validates every cached target
    /// before executing any compiler-proven scalar body.
    ScalarCallTree {
        guard: ScalarLongCallGuard,
        do_fcall_ip: usize,
        long_input_mask: u64,
        object_input_mask: u64,
        argument_count: u8,
        term_tmp: u16,
    },
}

/// A speculative edge that keeps an uncommon arbitrary PHP block outside a
/// typed hot region. A mismatch resumes the original pure comparison before
/// any cold-path instruction is skipped.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct QuickLongTraceGuard {
    pub kind: ScalarLongConditionKind,
    pub lhs: QuickLongOperand,
    pub rhs: QuickLongOperand,
    pub expected: bool,
    pub condition_tmp: Option<u16>,
    pub resume_ip: usize,
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
    pub tail_guard: Option<QuickLongTraceGuard>,
    pub increment_kind: QuickIncrementKind,
    pub increment_tmp: Option<u16>,
    pub sum_ip: usize,
    pub increment_ip: usize,
    #[cfg(all(
        feature = "jit-prototype",
        any(
            all(target_arch = "aarch64", target_os = "macos"),
            all(target_arch = "x86_64", target_os = "linux")
        )
    ))]
    native_jit: crate::jit::QuickLongAccumulateJitCache,
}

impl QuickLongAccumulateLoop {
    #[cfg(all(
        feature = "jit-prototype",
        any(
            all(target_arch = "aarch64", target_os = "macos"),
            all(target_arch = "x86_64", target_os = "linux")
        )
    ))]
    #[inline(always)]
    pub fn native_jit(&self) -> &crate::jit::QuickLongAccumulateJitCache {
        &self.native_jit
    }
}

#[derive(Debug, Clone, Copy, PartialEq)]
pub enum QuickDoubleSource {
    Input(u8),
    Induction,
    Constant(f64),
    Temporary(u8),
}

#[derive(Debug, Clone, Copy)]
pub struct QuickDoubleArgumentOp {
    pub kind: crate::vm::function::ScalarDoubleOpKind,
    pub lhs: QuickDoubleSource,
    pub rhs: QuickDoubleSource,
}

#[derive(Debug, Clone)]
pub struct QuickDoubleArgumentProgram {
    pub operations: Box<[QuickDoubleArgumentOp]>,
    pub outputs: [QuickDoubleSource; 8],
    pub output_count: u8,
    /// Exact-Double caller CVs bound to `QuickDoubleSource::Input` indices.
    pub input_slots: [u16; 8],
    pub input_count: u8,
}

impl QuickDoubleArgumentProgram {
    #[inline]
    pub(crate) fn source_depends_on_induction(&self, source: QuickDoubleSource) -> bool {
        match source {
            QuickDoubleSource::Induction => true,
            QuickDoubleSource::Temporary(index) => self
                .operations
                .get(index as usize)
                .is_some_and(|operation| {
                    self.source_depends_on_induction(operation.lhs)
                        || self.source_depends_on_induction(operation.rhs)
                }),
            QuickDoubleSource::Input(_) | QuickDoubleSource::Constant(_) => false,
        }
    }

    #[inline]
    pub(crate) fn operation_is_needed_by_output_phase(
        &self,
        operation: usize,
        induction_dependent: bool,
    ) -> bool {
        self.outputs[..self.output_count as usize]
            .iter()
            .copied()
            .filter(|output| {
                self.source_depends_on_induction(*output) == induction_dependent
            })
            .any(|output| self.source_uses_operation(output, operation))
    }

    #[inline]
    fn source_uses_operation(&self, source: QuickDoubleSource, expected: usize) -> bool {
        let QuickDoubleSource::Temporary(index) = source else {
            return false;
        };
        let index = index as usize;
        index == expected
            || self.operations.get(index).is_some_and(|operation| {
                self.source_uses_operation(operation.lhs, expected)
                    || self.source_uses_operation(operation.rhs, expected)
            })
    }

    /// Public arguments whose induction-dependent values can remain in their
    /// argument-program temporary until the scalar leaf consumes them.
    ///
    /// Argument and leaf temporaries deliberately share the native register
    /// bank. A value produced in temporary `n` is therefore safe to forward
    /// only while leaf operation `n` has not overwritten that register. The
    /// same-operation RHS restriction also keeps this proof valid for the
    /// two-operand x86-64 lowering, which writes the LHS into its destination
    /// before consuming the RHS.
    #[cfg(any(
        test,
        all(
            feature = "jit-prototype",
            any(
                all(target_arch = "aarch64", target_os = "macos"),
                all(target_arch = "x86_64", target_os = "linux")
            )
        )
    ))]
    pub(crate) fn register_forwardable_output_mask(
        &self,
        leaf: &crate::vm::function::ScalarDoubleFunctionPlan,
    ) -> u8 {
        // Conditional leaves may consume an input in the predicate or either
        // edge. Keep the first vertical slice conservative and materialize
        // every dynamic argument before entering the leaf branch.
        if leaf.select.is_some() {
            return 0;
        }
        let mut mask = 0_u8;
        for (argument, output) in self.outputs[..self.output_count as usize]
            .iter()
            .copied()
            .enumerate()
        {
            let QuickDoubleSource::Temporary(register) = output else {
                continue;
            };
            if !self.source_depends_on_induction(output) {
                continue;
            }

            let argument = argument as u16;
            let overwrite = register as usize;
            let mut safe = true;
            for (operation_index, operation) in
                leaf.program.operations.iter().copied().enumerate()
            {
                let lhs_uses_argument = matches!(
                    operation.lhs,
                    crate::vm::function::ScalarDoubleSource::Input(index)
                        if index == argument
                );
                let rhs_uses_argument = matches!(
                    operation.rhs,
                    crate::vm::function::ScalarDoubleSource::Input(index)
                        if index == argument
                );
                if (lhs_uses_argument && operation_index > overwrite)
                    || (rhs_uses_argument
                        && (operation_index > overwrite
                            || (operation_index == overwrite && !lhs_uses_argument)))
                {
                    safe = false;
                    break;
                }
            }
            if safe
                && matches!(
                    leaf.program.output,
                    crate::vm::function::ScalarDoubleSource::Input(index)
                        if index == argument
                )
                && leaf.program.operations.len() > overwrite
            {
                safe = false;
            }
            if safe {
                mask |= 1_u8 << argument;
            }
        }
        mask
    }
}

/// Borrowed view of a guarded Double callee after runtime resolution. The
/// program may be the callee's original flat leaf or a recursively flattened
/// composed body; the outer composer deliberately does not distinguish them.
#[derive(Clone, Copy)]
pub(crate) struct ResolvedScalarDoubleProgram<'a> {
    pub public_args: u8,
    pub program: &'a crate::vm::function::ScalarDoubleProgram,
    pub select: Option<crate::vm::function::ScalarDoubleSelect>,
}

/// Flatten one guarded composed Double body after its direct callees have been
/// resolved. Operation-result remapping is target-neutral: both native
/// backends receive the same established eight-temporary scalar program.
pub(crate) fn compose_scalar_double_program(
    plan: &crate::vm::function::ComposedScalarDoubleFunctionPlan,
    resolved_programs: &[Option<ResolvedScalarDoubleProgram<'_>>],
) -> Option<crate::vm::function::ScalarDoubleFunctionPlan> {
    use crate::vm::function::{
        ComposedScalarDoubleOp, ScalarDoubleFunctionPlan, ScalarDoubleOp, ScalarDoubleProgram,
        ScalarDoubleSelect, ScalarDoubleSource,
    };

    const MAX_OPERATIONS: usize = 8;
    if plan.operations.len() > 16
        || plan.operations.len() > resolved_programs.len()
    {
        return None;
    }

    fn remap_composed_source(
        source: ScalarDoubleSource,
        results: &[Option<ScalarDoubleSource>],
    ) -> Option<ScalarDoubleSource> {
        match source {
            ScalarDoubleSource::Input(index) => Some(ScalarDoubleSource::Input(index)),
            ScalarDoubleSource::Constant(value) => {
                Some(ScalarDoubleSource::Constant(value))
            }
            ScalarDoubleSource::Temporary(index) => {
                results.get(index as usize).copied().flatten()
            }
            ScalarDoubleSource::Selection => Some(ScalarDoubleSource::Selection),
        }
    }

    let mut operations = Vec::with_capacity(MAX_OPERATIONS);
    let mut results = [None; 16];
    let mut merged_select = None;
    for (composed_index, operation) in plan.operations.iter().enumerate() {
        results[composed_index] = Some(match operation {
            ComposedScalarDoubleOp::Arithmetic(operation) => {
                if operations.len() == MAX_OPERATIONS {
                    return None;
                }
                let lhs = remap_composed_source(operation.lhs, &results)?;
                let rhs = remap_composed_source(operation.rhs, &results)?;
                let result = ScalarDoubleSource::Temporary(operations.len() as u8);
                operations.push(ScalarDoubleOp {
                    kind: operation.kind,
                    lhs,
                    rhs,
                });
                result
            }
            ComposedScalarDoubleOp::Call(call) => {
                let resolved = resolved_programs
                    .get(composed_index)
                    .copied()
                    .flatten()?;
                if resolved.public_args as usize != call.arguments.len()
                    || operations.len() + resolved.program.operations.len()
                        > MAX_OPERATIONS
                {
                    return None;
                }
                if call.arguments.len() > 8 {
                    return None;
                }
                let mut arguments = [ScalarDoubleSource::Constant(0.0); 8];
                for (index, source) in call.arguments.iter().copied().enumerate() {
                    arguments[index] = remap_composed_source(source, &results)?;
                }
                let leaf_start = operations.len();
                let remap_leaf_source = |source| match source {
                    ScalarDoubleSource::Input(index)
                        if (index as usize) < call.arguments.len() =>
                    {
                        Some(arguments[index as usize])
                    }
                    ScalarDoubleSource::Input(_) => None,
                    ScalarDoubleSource::Constant(value) => {
                        Some(ScalarDoubleSource::Constant(value))
                    }
                    ScalarDoubleSource::Temporary(index) => leaf_start
                        .checked_add(index as usize)
                        .filter(|index| {
                            *index < leaf_start + resolved.program.operations.len()
                        })
                        .and_then(|index| u8::try_from(index).ok())
                        .map(ScalarDoubleSource::Temporary),
                    ScalarDoubleSource::Selection => resolved
                        .select
                        .is_some()
                        .then_some(ScalarDoubleSource::Selection),
                };
                for operation in resolved.program.operations.iter().copied() {
                    operations.push(ScalarDoubleOp {
                        kind: operation.kind,
                        lhs: remap_leaf_source(operation.lhs)?,
                        rhs: remap_leaf_source(operation.rhs)?,
                    });
                }
                if let Some(select) = resolved.select {
                    if merged_select.is_some() {
                        return None;
                    }
                    let (shared_end, _, _) =
                        select.operation_ranges(resolved.program.operations.len())?;
                    merged_select = Some(ScalarDoubleSelect {
                        kind: select.kind,
                        lhs: remap_leaf_source(select.lhs)?,
                        rhs: remap_leaf_source(select.rhs)?,
                        shared_operation_count: u8::try_from(leaf_start.checked_add(shared_end)?)
                            .ok()?,
                        when_true_operation_count: select.when_true_operation_count,
                        when_false_operation_count: select.when_false_operation_count,
                        when_true: remap_leaf_source(select.when_true)?,
                        when_false: remap_leaf_source(select.when_false)?,
                        merge_result: true,
                    });
                    if select.merge_result {
                        remap_leaf_source(resolved.program.output)?
                    } else {
                        ScalarDoubleSource::Selection
                    }
                } else {
                    remap_leaf_source(resolved.program.output)?
                }
            }
        });
    }

    let program = ScalarDoubleProgram {
        operations: operations.into_boxed_slice(),
        output: remap_composed_source(plan.output, &results)?,
    };
    Some(if let Some(select) = merged_select {
        ScalarDoubleFunctionPlan::new_conditional(plan.public_args, program, select)
    } else {
        ScalarDoubleFunctionPlan::new(plan.public_args, program)
    })
}

/// A mixed Long-control/Double-data loop whose hot body is one exact-Double
/// scalar call followed by accumulation. The callee body remains represented
/// by its target-neutral ScalarDoubleProgram; this plan describes only the
/// caller bindings and precise baseline resume points.
#[derive(Debug)]
pub struct QuickDoubleCallAccumulateLoop {
    pub header_ip: usize,
    pub exit_ip: usize,
    pub induction_cv: u16,
    pub bound: QuickLongBound,
    pub accumulator_cv: u16,
    pub condition_tmp: Option<u16>,
    pub guard: ScalarLongCallGuard,
    pub argument_program: QuickDoubleArgumentProgram,
    pub typed_invariant_source: Option<QuickTypedInvariantSource>,
    pub term_tmp: u16,
    pub sum_tmp: u16,
    pub increment_kind: QuickIncrementKind,
    pub increment_tmp: Option<u16>,
    pub sum_ip: usize,
    pub increment_ip: usize,
    #[cfg(all(
        feature = "jit-prototype",
        any(
            all(target_arch = "aarch64", target_os = "macos"),
            all(target_arch = "x86_64", target_os = "linux")
        )
    ))]
    native_jit: crate::jit::QuickDoubleCallAccumulateJitCache,
}

impl QuickDoubleCallAccumulateLoop {
    #[cfg(all(
        feature = "jit-prototype",
        any(
            all(target_arch = "aarch64", target_os = "macos"),
            all(target_arch = "x86_64", target_os = "linux")
        )
    ))]
    #[inline(always)]
    pub fn native_jit(&self) -> &crate::jit::QuickDoubleCallAccumulateJitCache {
        &self.native_jit
    }
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

/// Shared guarded ABI for every frame-free typed method call in a general
/// scalar region. Executable variants retain their proven body kind so the hot
/// loop does not redispatch it on every call.
#[derive(Debug, Clone, Copy)]
pub struct QuickTypedMethodCall {
    pub guard: ScalarLongCallGuard,
    pub arguments: [QuickLongOperand; 8],
    pub argument_count: u8,
    pub next_target: QuickLongTarget,
    pub resume_ip: usize,
}

/// Positional input to a read-only mixed scalar method. The enclosing region
/// retains Longs unboxed and immutable Strings as borrowed slot state.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum QuickObjectLongArgument {
    Long(QuickLongOperand),
    StringSlot(u16),
}

#[derive(Debug, Clone, Copy)]
pub struct QuickObjectLongMethodCall {
    pub guard: ScalarLongCallGuard,
    pub arguments: [QuickObjectLongArgument; 8],
    pub argument_count: u8,
    pub next_target: QuickLongTarget,
    pub resume_ip: usize,
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

/// Input source retained by a loop-invariant typed producer.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum QuickInvariantInput {
    StringSlot(u16),
    StringLiteral(u16),
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum QuickInvariantPathElement {
    StringLiteral(u16),
    Integer(i64),
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum QuickInvariantValueKind {
    Long,
    Double,
    String,
    /// Byte length derived from an exact String projection. The projected
    /// frame result is a Long, so scalar consumers need no string operation.
    StringLength,
}

#[derive(Debug, Clone)]
pub struct QuickTypedInvariantProjection {
    pub path: Box<[QuickInvariantPathElement]>,
    pub result: u16,
    pub kind: QuickInvariantValueKind,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum QuickTypedInvariantProducer {
    JsonDecodeAssociative { input: QuickInvariantInput },
}

/// One loop-invariant producer whose non-escaping result is consumed through
/// fixed, exactly typed projections inside a guarded region.
#[derive(Debug, Clone)]
pub struct QuickTypedInvariantSource {
    pub producer: QuickTypedInvariantProducer,
    pub destination: u16,
    pub projections: Vec<QuickTypedInvariantProjection>,
    pub long_output_mask: u64,
    pub double_output_mask: u64,
    pub string_output_mask: u64,
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
    BranchUnlessLe {
        lhs: QuickLongOperand,
        rhs: QuickLongOperand,
        condition_tmp: Option<u16>,
        false_target: QuickLongTarget,
        next_target: QuickLongTarget,
        resume_ip: usize,
    },
    /// Speculatively take one structurally selected hot edge while leaving the
    /// skipped arbitrary PHP range in canonical bytecode. A mismatch resumes
    /// the original comparison before any cold instruction is skipped.
    TraceGuard {
        kind: ScalarLongConditionKind,
        lhs: QuickLongOperand,
        rhs: QuickLongOperand,
        expected: bool,
        condition_tmp: Option<u16>,
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
    /// Baseline position consumed by the loop-invariant JSON projection
    /// prelude. Quick/native execution treats this as a zero-cost control edge.
    JsonProjectionStep {
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
    /// Guarded read of an invariant receiver's Long property. Resolution
    /// binds the current property location once at typed-region entry.
    ObjectPropertyLong {
        object: u16,
        cache_ip: u32,
        result: u16,
        next_target: QuickLongTarget,
        resume_ip: usize,
    },
    /// Guarded borrowed strlen of an invariant receiver's String property.
    /// The intermediate heap Value is never materialized in the frame.
    ObjectPropertyStringLength {
        object: u16,
        cache_ip: u32,
        result: u16,
        next_target: QuickLongTarget,
        resume_ip: usize,
    },
    /// Replace an existing Long array entry selected by the same normalized
    /// integer/string key rules as canonical AssignDim. Planning admits this
    /// only when a preceding guarded fetch proved that the key exists.
    StoreArrayLong {
        array: u16,
        index: QuickArrayIndex,
        value: u16,
        next_target: QuickLongTarget,
        resume_ip: usize,
    },
    /// Append a retained Long value to a unique COW array. Runtime resolves
    /// the mutable packed/hash storage once at region entry.
    ArrayPushLong {
        array: u16,
        value: QuickLongOperand,
        next_target: QuickLongTarget,
        resume_ip: usize,
    },
    /// Append a guarded literal or invariant string CV to a unique COW string.
    StringAppend {
        destination: u16,
        source: QuickStringAppendSource,
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
    /// Checked scalar arithmetic when either operand may be a literal. The
    /// older Add variants retain their denser accumulator fusions.
    Binary {
        kind: ScalarLongOpKind,
        lhs: QuickLongOperand,
        rhs: QuickLongOperand,
        result: u16,
        next_target: QuickLongTarget,
        resume_ip: usize,
    },
    /// General checked arithmetic immediately materialized into a CV. Unlike
    /// AddAssign this also admits literals, subtraction, and multiplication.
    BinaryAssign {
        kind: ScalarLongOpKind,
        lhs: QuickLongOperand,
        rhs: QuickLongOperand,
        result: u16,
        destination: u16,
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
    /// Frame-free compiler-proven property mutator.
    PropertyMethodCall {
        call: QuickTypedMethodCall,
    },
    /// Compiler-proven declared-property getter.
    PropertyGetterCall {
        call: QuickTypedMethodCall,
        result: u16,
    },
    /// Monomorphic pure scalar method with Long result.
    ScalarMethodCall {
        call: QuickTypedMethodCall,
        result: u16,
    },
    /// Monomorphic read-only method with mixed Long/String inputs and a Long
    /// result, executed through ObjectLongFunctionPlan without a PHP frame.
    ObjectLongMethodCall {
        call: QuickObjectLongMethodCall,
        result: u16,
    },
    /// Exact PHP evaluation order for `propertyMutator(propertyGetter())`.
    ComposedPropertyCall {
        outer_guard: ScalarLongCallGuard,
        inner_guard: ScalarLongCallGuard,
        next_target: QuickLongTarget,
        resume_ip: usize,
    },
    /// A compiler-proven non-escaping constructor → ObjectArray call whose
    /// immediate scalar consumers are part of this closed typed region.
    VirtualObjectArrayPipeline {
        constructor_arguments: [QuickVirtualValueSource; 8],
        argument_count: u8,
        consumers: [QuickObjectArrayConsumer; 4],
        consumer_count: u8,
        trailing_key_literal: Option<u16>,
        trailing_result: u16,
        output_mask: u64,
        next_target: QuickLongTarget,
        resume_ip: usize,
    },
    Assign {
        destination: u16,
        source: u16,
        next_target: QuickLongTarget,
    },
    AssignLongLiteral {
        destination: u16,
        value: i64,
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
            }
            | Self::BranchUnlessLe {
                false_target,
                next_target,
                ..
            } => {
                resolve(false_target)?;
                resolve(next_target)
            }
            Self::ModConst { next_target, .. }
            | Self::JsonProjectionStep { next_target, .. }
            | Self::TraceGuard { next_target, .. }
            | Self::FetchArrayLong { next_target, .. }
            | Self::ObjectPropertyLong { next_target, .. }
            | Self::ObjectPropertyStringLength { next_target, .. }
            | Self::StoreArrayLong { next_target, .. }
            | Self::ArrayPushLong { next_target, .. }
            | Self::StringAppend { next_target, .. }
            | Self::Add { next_target, .. }
            | Self::Binary { next_target, .. }
            | Self::BinaryAssign { next_target, .. }
            | Self::AddAssign { next_target, .. }
            | Self::ConditionalAddAssign { next_target, .. }
            | Self::AddAddAssign { next_target, .. }
            | Self::ComposedPropertyCall { next_target, .. }
            | Self::VirtualObjectArrayPipeline { next_target, .. }
            | Self::Assign { next_target, .. }
            | Self::AssignLongLiteral { next_target, .. }
            | Self::AssignStringLiteral { next_target, .. }
            | Self::AssignStringSlot { next_target, .. }
            | Self::PostInc { next_target, .. } => resolve(next_target),
            Self::PropertyMethodCall { call }
            | Self::PropertyGetterCall { call, .. }
            | Self::ScalarMethodCall { call, .. } => resolve(&mut call.next_target),
            Self::ObjectLongMethodCall { call, .. } => resolve(&mut call.next_target),
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
    pub array_update_fusions: Vec<Option<QuickArrayUpdateFusion>>,
    pub entry_op: u16,
    op_ips: Vec<u32>,
    pub long_input_mask: u64,
    pub long_output_mask: u64,
    pub bool_output_mask: u64,
    pub array_input_mask: u64,
    pub array_output_mask: u64,
    pub string_input_mask: u64,
    pub string_output_mask: u64,
    pub string_append_mask: u64,
    pub object_input_mask: u64,
    pub typed_invariant_source: Option<QuickTypedInvariantSource>,
    pub string_cache_capacity: u8,
    pub involved_mask: u64,
    pub straight_array_kernel: Option<QuickStraightArrayRegionKernel>,
    #[cfg(all(
        feature = "jit-prototype",
        any(
            all(target_arch = "aarch64", target_os = "macos"),
            all(target_arch = "x86_64", target_os = "linux")
        )
    ))]
    native_jit: crate::jit::QuickLongOpsJitCache,
}

impl QuickLongOpsLoop {
    #[inline(always)]
    pub fn target_ip(&self, target: QuickLongTarget) -> Option<usize> {
        match target.op_index() {
            Some(index) => self.op_ips.get(index).copied().map(|ip| ip as usize),
            None => target.exit_ip(),
        }
    }

    #[cfg(all(
        feature = "jit-prototype",
        any(
            all(target_arch = "aarch64", target_os = "macos"),
            all(target_arch = "x86_64", target_os = "linux")
        )
    ))]
    #[inline(always)]
    pub fn native_jit(&self) -> &crate::jit::QuickLongOpsJitCache {
        &self.native_jit
    }
}

fn detect_array_update_fusions(
    ops: &[QuickLongOp],
) -> Vec<Option<QuickArrayUpdateFusion>> {
    let mut fusions = vec![None; ops.len()];
    for index in 0..ops.len().saturating_sub(2) {
        let QuickLongOp::FetchArrayLong {
            array,
            index: array_index,
            result: fetch_result,
            next_target: fetch_next,
            ..
        } = ops[index]
        else {
            continue;
        };
        if fetch_next.op_index() != Some(index + 1)
            || ops.iter().any(|operation| {
                matches!(operation, QuickLongOp::ArrayPushLong { array: pushed, .. } if *pushed == array)
            })
        {
            continue;
        }

        let (kind, lhs, rhs, result, arithmetic_next, arithmetic_resume_ip) =
            match ops[index + 1] {
                QuickLongOp::Add {
                    lhs,
                    rhs,
                    result,
                    next_target,
                    resume_ip,
                } => (
                    ScalarLongOpKind::Add,
                    QuickLongOperand::Slot(lhs),
                    QuickLongOperand::Slot(rhs),
                    result,
                    next_target,
                    resume_ip,
                ),
                QuickLongOp::Binary {
                    kind,
                    lhs,
                    rhs,
                    result,
                    next_target,
                    resume_ip,
                } => (kind, lhs, rhs, result, next_target, resume_ip),
                _ => continue,
            };
        if arithmetic_next.op_index() != Some(index + 2)
            || !matches!(lhs, QuickLongOperand::Slot(slot) if slot == fetch_result)
                && !matches!(rhs, QuickLongOperand::Slot(slot) if slot == fetch_result)
        {
            continue;
        }

        let QuickLongOp::StoreArrayLong {
            array: store_array,
            index: store_index,
            value,
            next_target,
            ..
        } = ops[index + 2]
        else {
            continue;
        };
        if store_array != array || store_index != array_index || value != result {
            continue;
        }
        fusions[index] = Some(QuickArrayUpdateFusion {
            kind,
            lhs,
            rhs,
            result,
            next_target,
            arithmetic_resume_ip,
        });
    }
    fusions
}

pub const QUICK_STRAIGHT_ARRAY_MAX_ADDS: usize = 4;

fn instruction_mentions_operand(
    instruction: &crate::vm::instruction::Instruction,
    op_type: OpType,
    slot: u16,
) -> bool {
    (instruction.op1_type == op_type && instruction.op1 == slot)
        || (instruction.op2_type == op_type && instruction.op2 == slot)
        || (instruction.result_type == op_type && instruction.result == slot)
}

fn object_array_add_consumer(
    fetch: crate::vm::instruction::Instruction,
    add: crate::vm::instruction::Instruction,
    assign: crate::vm::instruction::Instruction,
) -> Option<u16> {
    if !matches!(add.opcode, OpCode::Add | OpCode::Add_CvTmp | OpCode::Add_TmpTmp)
        || !matches!(add.result_type, OpType::Tmp | OpType::Var)
        || assign.opcode != OpCode::AssignCv
        || assign.op1_type != OpType::Cv
        || assign.op2_type != add.result_type
        || assign.op2 != add.result
        || assign.result_type != OpType::Unused
    {
        return None;
    }
    let accumulator = if add.op1_type == OpType::Cv
        && add.op2_type == fetch.result_type
        && add.op2 == fetch.result
    {
        add.op1
    } else if add.op2_type == OpType::Cv
        && add.op1_type == fetch.result_type
        && add.op1 == fetch.result
    {
        add.op2
    } else {
        return None;
    };
    (assign.op1 == accumulator).then_some(accumulator)
}

/// Prove an immediate scalar-consumer span for a method's small associative
/// array result. The assigned array CV must have no other syntactic use in the
/// function, which makes non-materialization unobservable for the admitted
/// Long-only ObjectArrayFunctionPlan result.
pub fn detect_object_array_consumer_span(
    op_array: &OpArray,
    init_ip: usize,
) -> Option<usize> {
    let initializer = *op_array.instructions.get(init_ip)?;
    if initializer.opcode != OpCode::InitMethodCall {
        return None;
    }
    let do_fcall_ip = init_ip
        .checked_add(1)?
        .checked_add(initializer.extended_value as usize)?;
    let do_fcall = *op_array.instructions.get(do_fcall_ip)?;
    let assign_ip = do_fcall_ip + 1;
    let assign = *op_array.instructions.get(assign_ip)?;
    if do_fcall.opcode != OpCode::DoFcall
        || !matches!(do_fcall.result_type, OpType::Tmp | OpType::Var)
        || assign.opcode != OpCode::AssignCv
        || assign.op1_type != OpType::Cv
        || assign.op2_type != do_fcall.result_type
        || assign.op2 != do_fcall.result
        || assign.result_type != OpType::Unused
    {
        return None;
    }

    let array_cv = assign.op1;
    let mut fetch_ips = [usize::MAX; QUICK_STRAIGHT_ARRAY_MAX_ADDS];
    let mut fetch_count = 0usize;
    let mut add_count = 0usize;
    let mut cursor = assign_ip + 1;
    while fetch_count < fetch_ips.len() {
        let Some(fetch) = op_array.instructions.get(cursor).copied() else {
            break;
        };
        if fetch.opcode != OpCode::FetchDimR
            || fetch.op1_type != OpType::Cv
            || fetch.op1 != array_cv
            || fetch.op2_type != OpType::Const
            || !matches!(fetch.result_type, OpType::Tmp | OpType::Var)
            || op_array
                .literals
                .get(fetch.op2 as usize)
                .and_then(Value::as_str)
                .is_none()
        {
            break;
        }
        fetch_ips[fetch_count] = cursor;
        fetch_count += 1;

        let add = op_array.instructions.get(cursor + 1).copied();
        let assign = op_array.instructions.get(cursor + 2).copied();
        if let (Some(add), Some(assign)) = (add, assign)
            && object_array_add_consumer(fetch, add, assign).is_some()
        {
            add_count += 1;
            cursor += 3;
            continue;
        }

        // One final fetch may feed arbitrary canonical scalar bytecode. The
        // fast path materializes that Long TMP and resumes immediately after
        // the fetch.
        cursor += 1;
        break;
    }
    if add_count == 0 || fetch_count == 0 || cursor >= op_array.instructions.len() {
        return None;
    }

    for (ip, instruction) in op_array.instructions.iter().enumerate() {
        if ip == assign_ip || fetch_ips[..fetch_count].contains(&ip) {
            continue;
        }
        if instruction_mentions_operand(instruction, OpType::Cv, array_cv) {
            return None;
        }
        if ip != do_fcall_ip
            && ip != assign_ip
            && instruction_mentions_operand(
                instruction,
                do_fcall.result_type,
                do_fcall.result,
            )
        {
            return None;
        }
    }

    Some(cursor)
}

/// Prove the caller-side escape shape for a constructor-initialized object
/// passed directly into an ObjectArray consumer call. Runtime supplies the
/// class, constructor-plan and declared-property guards that are unavailable
/// while this caller is compiled.
pub fn detect_virtual_object_array_pipeline_span(
    op_array: &OpArray,
    new_ip: usize,
) -> Option<usize> {
    let new_object = *op_array.instructions.get(new_ip)?;
    if new_object.opcode != OpCode::NewObj
        || new_object.op1_type != OpType::Const
        || !matches!(new_object.result_type, OpType::Tmp | OpType::Var)
        || new_object.extended_value == 0
        || new_object.extended_value > 8
        || op_array
            .literals
            .get(new_object.op1 as usize)
            .and_then(Value::as_str)
            .is_none()
    {
        return None;
    }
    let constructor_do_ip = new_ip + 1 + new_object.extended_value as usize;
    let constructor_do = *op_array.instructions.get(constructor_do_ip)?;
    let object_assign_ip = constructor_do_ip + 1;
    let object_assign = *op_array.instructions.get(object_assign_ip)?;
    if constructor_do.opcode != OpCode::DoFcall
        || object_assign.opcode != OpCode::AssignCv
        || object_assign.op1_type != OpType::Cv
        || object_assign.op2_type != new_object.result_type
        || object_assign.op2 != new_object.result
        || object_assign.result_type != OpType::Unused
    {
        return None;
    }

    for index in 0..new_object.extended_value as usize {
        let send = *op_array.instructions.get(new_ip + 1 + index)?;
        if !matches!(send.opcode, OpCode::SendVal | OpCode::SendVarEx) {
            return None;
        }
    }

    let method_ip = object_assign_ip + 1;
    let method = *op_array.instructions.get(method_ip)?;
    if method.opcode != OpCode::InitMethodCall
        || method._pad & crate::vm::instruction::CALL_FLAG_OBJECT_ARRAY_CONSUMERS == 0
        || detect_object_array_consumer_span(op_array, method_ip).is_none()
    {
        return None;
    }
    let mut virtual_argument_sends = 0usize;
    let mut virtual_send_ip = usize::MAX;
    for index in 0..method.extended_value as usize {
        let send_ip = method_ip + 1 + index;
        let send = *op_array.instructions.get(send_ip)?;
        if !matches!(send.opcode, OpCode::SendVal | OpCode::SendVarEx) {
            return None;
        }
        if send.op1_type == OpType::Cv && send.op1 == object_assign.op1 {
            virtual_argument_sends += 1;
            virtual_send_ip = send_ip;
        }
    }
    if virtual_argument_sends != 1 {
        return None;
    }

    for (ip, instruction) in op_array.instructions.iter().enumerate() {
        if ip != new_ip
            && ip != object_assign_ip
            && instruction_mentions_operand(
                instruction,
                new_object.result_type,
                new_object.result,
            )
        {
            return None;
        }
        if ip != object_assign_ip
            && ip != virtual_send_ip
            && instruction_mentions_operand(instruction, OpType::Cv, object_assign.op1)
        {
            return None;
        }
        if matches!(constructor_do.result_type, OpType::Tmp | OpType::Var)
            && ip != constructor_do_ip
            && instruction_mentions_operand(
                instruction,
                constructor_do.result_type,
                constructor_do.result,
            )
        {
            return None;
        }
    }

    detect_object_array_consumer_span(op_array, method_ip)
}

#[derive(Debug, Clone, Copy)]
pub struct QuickStraightArrayFetchAdd {
    pub index: QuickArrayIndex,
    pub fetch_result: u16,
    pub accumulator: u16,
    pub add_result: u16,
    pub fetch_resume_ip: usize,
    pub add_resume_ip: usize,
}

impl QuickStraightArrayFetchAdd {
    const EMPTY: Self = Self {
        index: QuickArrayIndex::Long(QuickLongOperand::Const(0)),
        fetch_result: 0,
        accumulator: 0,
        add_result: 0,
        fetch_resume_ip: 0,
        add_resume_ip: 0,
    };
}

#[derive(Debug, Clone, Copy)]
pub struct QuickStraightArrayFetch {
    pub index: QuickArrayIndex,
    pub result: u16,
    pub resume_ip: usize,
}

#[derive(Debug, Clone, Copy)]
pub struct QuickStraightArrayRegionKernel {
    pub array: u16,
    pub adds: [QuickStraightArrayFetchAdd; QUICK_STRAIGHT_ARRAY_MAX_ADDS],
    pub add_count: u8,
    pub trailing_fetch: Option<QuickStraightArrayFetch>,
    pub exit_target: QuickLongTarget,
}

/// Select a measured superinstruction from the general typed graph. PHP names,
/// literal keys, and source-level workload identity never participate; this
/// only compresses adjacent FetchArrayLong/AddAssign operations over one
/// immutable result array.
fn detect_straight_array_region_kernel(
    plan: &QuickLongOpsLoop,
) -> Option<QuickStraightArrayRegionKernel> {
    if plan.entry_op != 0
        || plan.ops.len() < 2
        || plan.array_input_mask.count_ones() != 1
        || plan.array_output_mask != 0
        || plan.string_input_mask != 0
        || plan.string_output_mask != 0
        || plan.string_append_mask != 0
        || plan.object_input_mask != 0
    {
        return None;
    }

    let array = plan.array_input_mask.trailing_zeros() as u16;
    let mut adds = [QuickStraightArrayFetchAdd::EMPTY; QUICK_STRAIGHT_ARRAY_MAX_ADDS];
    let mut add_count = 0usize;
    let mut cursor = 0usize;
    let mut exit_target = None;

    while cursor + 1 < plan.ops.len() && add_count < adds.len() {
        let QuickLongOp::FetchArrayLong {
            array: fetch_array,
            index,
            result: fetch_result,
            destination: None,
            next_target: fetch_next,
            resume_ip: fetch_resume_ip,
        } = plan.ops[cursor]
        else {
            break;
        };
        if fetch_array != array
            || matches!(index, QuickArrayIndex::ValueSlot(_))
            || fetch_next.op_index() != Some(cursor + 1)
        {
            return None;
        }
        let QuickLongOp::AddAssign {
            lhs,
            rhs,
            result: add_result,
            destination,
            next_target,
            add_resume_ip,
        } = plan.ops[cursor + 1]
        else {
            break;
        };
        let accumulator = if lhs == destination && rhs == fetch_result {
            lhs
        } else if rhs == destination && lhs == fetch_result {
            rhs
        } else {
            return None;
        };
        adds[add_count] = QuickStraightArrayFetchAdd {
            index,
            fetch_result,
            accumulator,
            add_result,
            fetch_resume_ip,
            add_resume_ip,
        };
        add_count += 1;
        cursor += 2;
        if cursor < plan.ops.len() {
            if next_target.op_index() != Some(cursor) {
                return None;
            }
        } else {
            exit_target = Some(next_target);
        }
    }

    let trailing_fetch = if cursor < plan.ops.len() {
        if cursor + 1 != plan.ops.len() {
            return None;
        }
        let QuickLongOp::FetchArrayLong {
            array: fetch_array,
            index,
            result,
            destination: None,
            next_target,
            resume_ip,
        } = plan.ops[cursor]
        else {
            return None;
        };
        if fetch_array != array || matches!(index, QuickArrayIndex::ValueSlot(_)) {
            return None;
        }
        exit_target = Some(next_target);
        Some(QuickStraightArrayFetch {
            index,
            result,
            resume_ip,
        })
    } else {
        None
    };

    if add_count == 0 {
        return None;
    }
    let exit_target = exit_target?;
    exit_target.exit_ip()?;
    Some(QuickStraightArrayRegionKernel {
        array,
        adds,
        add_count: add_count as u8,
        trailing_fetch,
        exit_target,
    })
}

fn long_literal(op_array: &OpArray, index: u16) -> Option<i64> {
    op_array
        .literals
        .get(index as usize)
        .and_then(Value::as_long)
}

#[derive(Clone, Copy)]
struct ProvenQuickDoubleSource {
    source: QuickDoubleSource,
    is_double: bool,
}

fn quick_double_argument_source(
    op_array: &OpArray,
    op_type: OpType,
    operand: u16,
    induction_cv: u16,
    produced_temporary_slots: &[u16; 8],
    produced_temporary_count: usize,
    input_slots: &mut [u16; 8],
    input_count: &mut usize,
    double_input_mask: &mut u64,
    total_slots: u32,
) -> Option<ProvenQuickDoubleSource> {
    match op_type {
        OpType::Cv if operand == induction_cv => Some(ProvenQuickDoubleSource {
            source: QuickDoubleSource::Induction,
            is_double: false,
        }),
        OpType::Cv => {
            add_mask_slot(double_input_mask, operand, total_slots)?;
            let input = if let Some(index) = input_slots[..*input_count]
                .iter()
                .position(|slot| *slot == operand)
            {
                index
            } else {
                if *input_count == input_slots.len() {
                    return None;
                }
                let index = *input_count;
                input_slots[index] = operand;
                *input_count += 1;
                index
            };
            Some(ProvenQuickDoubleSource {
                source: QuickDoubleSource::Input(input as u8),
                is_double: true,
            })
        }
        OpType::Const => {
            let value = op_array.literals.get(operand as usize)?;
            match value.value_type() {
                crate::value::ValueType::Double => Some(ProvenQuickDoubleSource {
                    source: QuickDoubleSource::Constant(value.as_double()?),
                    is_double: true,
                }),
                crate::value::ValueType::Long => Some(ProvenQuickDoubleSource {
                    source: QuickDoubleSource::Constant(value.as_long()? as f64),
                    is_double: false,
                }),
                _ => None,
            }
        }
        OpType::Tmp | OpType::Var => produced_temporary_slots
            [..produced_temporary_count]
            .iter()
            .position(|slot| *slot == operand)
            .map(|index| ProvenQuickDoubleSource {
                source: QuickDoubleSource::Temporary(index as u8),
                is_double: true,
            }),
        OpType::Unused => None,
    }
}

/// Recognize a closed `Long induction + Double scalar leaf + Double
/// accumulation` loop. Caller argument expressions are retained as a small
/// target-neutral Double program; exact-Double CVs are guarded at entry and
/// the Long induction value is converted by the selected execution tier.
pub fn detect_double_call_accumulate_loop(
    op_array: &OpArray,
    header_ip: usize,
    backedge_ip: usize,
) -> Option<QuickDoubleCallAccumulateLoop> {
    if header_ip.checked_add(8)? > backedge_ip || backedge_ip >= op_array.instructions.len() {
        return None;
    }
    let condition = *op_array.instructions.get(header_ip)?;
    let branch = *op_array.instructions.get(header_ip + 1)?;
    let backedge = *op_array.instructions.get(backedge_ip)?;
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

    let total_slots = op_array.num_cvs.checked_add(op_array.num_temps)?;
    if total_slots > 64 {
        return None;
    }
    let region = &op_array.instructions[header_ip..=backedge_ip];
    let mut initializer_ip = header_ip + 2;
    let mut typed_invariant_source = None;
    let mut json_paths: Vec<Option<Vec<QuickInvariantPathElement>>> =
        vec![None; total_slots as usize];
    let mut json_fetch_mask = 0u64;
    let mut json_parent_mask = 0u64;
    let possible_producer = *op_array.instructions.get(initializer_ip)?;
    if possible_producer.opcode == OpCode::DirectInternalCall2
        && crate::builtin_metadata::DirectInternalKind::from_id(
            possible_producer.extended_value,
        ) == Some(crate::builtin_metadata::DirectInternalKind::JsonDecode)
    {
        let source = detect_json_typed_invariant_source(op_array, region, initializer_ip)?;
        json_paths
            .get_mut(source.destination as usize)?
            .replace(Vec::new());
        typed_invariant_source = Some(source);
        initializer_ip += 2;
    }
    let initializer = *op_array.instructions.get(initializer_ip)?;
    let (argument_count, argument_offset, guard, receiver_slot) = match initializer.opcode {
        OpCode::InitFcall if initializer.op1 <= 8 => (
            initializer.op1 as usize,
            0usize,
            ScalarLongCallGuard::FunctionCache {
                cache_ip: u32::try_from(initializer_ip).ok()?,
            },
            None,
        ),
        OpCode::InitMethodCall
            if initializer.op1_type == OpType::Cv && initializer.extended_value <= 8 =>
        {
            (
                initializer.extended_value as usize,
                1usize,
                ScalarLongCallGuard::MethodCache {
                    cache_ip: u32::try_from(initializer_ip).ok()?,
                    receiver_slot: initializer.op1,
                },
                Some(initializer.op1),
            )
        }
        _ => return None,
    };
    let mut outputs = [QuickDoubleSource::Constant(0.0); 8];
    let mut operations = Vec::with_capacity(8);
    let mut produced_temporary_slots = [u16::MAX; 8];
    let mut expression_count = 0usize;
    let mut sent_arguments = 0usize;
    let mut input_slots = [u16::MAX; 8];
    let mut input_count = 0usize;
    let mut double_input_mask = 0u64;
    let mut cursor = initializer_ip + 1;
    while sent_arguments < argument_count {
        let send = *op_array.instructions.get(cursor)?;
        if send.opcode == OpCode::FetchDimR
            && matches!(send.op1_type, OpType::Cv | OpType::Tmp | OpType::Var)
            && send.result_type == OpType::Tmp
        {
            let Some(mut path) = json_paths
                .get(send.op1 as usize)
                .and_then(|path| path.as_ref())
                .cloned()
            else {
                return None;
            };
            let element = fixed_invariant_path_element(op_array, send.op2_type, send.op2)?;
            if path.len() == 8 {
                return None;
            }
            path.push(element);
            json_paths.get_mut(send.result as usize)?.replace(path);
            add_mask_slot(&mut json_fetch_mask, send.result, total_slots)?;
            add_mask_slot(&mut json_parent_mask, send.op1, total_slots)?;
            cursor += 1;
            continue;
        }
        if matches!(send.opcode, OpCode::SendVal | OpCode::SendVarEx) {
            if send.op2 as usize != sent_arguments + argument_offset {
                return None;
            }
            let output = if matches!(send.op1_type, OpType::Tmp | OpType::Var)
                && json_paths
                    .get(send.op1 as usize)
                    .and_then(|path| path.as_ref())
                    .is_some()
            {
                let path = json_paths.get(send.op1 as usize)?.as_ref()?.clone();
                if path.is_empty() {
                    return None;
                }
                add_mask_slot(&mut double_input_mask, send.op1, total_slots)?;
                let input = if let Some(index) = input_slots[..input_count]
                    .iter()
                    .position(|slot| *slot == send.op1)
                {
                    index
                } else {
                    if input_count == input_slots.len() {
                        return None;
                    }
                    let index = input_count;
                    input_slots[index] = send.op1;
                    input_count += 1;
                    index
                };
                let source = typed_invariant_source.as_mut()?;
                if source.double_output_mask & (1u64 << send.op1) == 0 {
                    source.double_output_mask |= 1u64 << send.op1;
                    source.projections.push(QuickTypedInvariantProjection {
                        path: path.into_boxed_slice(),
                        result: send.op1,
                        kind: QuickInvariantValueKind::Double,
                    });
                }
                ProvenQuickDoubleSource {
                    source: QuickDoubleSource::Input(input as u8),
                    is_double: true,
                }
            } else {
                quick_double_argument_source(
                    op_array,
                    send.op1_type,
                    send.op1,
                    induction_cv,
                    &produced_temporary_slots,
                    expression_count,
                    &mut input_slots,
                    &mut input_count,
                    &mut double_input_mask,
                    total_slots,
                )?
            };
            if !output.is_double {
                return None;
            }
            outputs[sent_arguments] = output.source;
            sent_arguments += 1;
            cursor += 1;
            continue;
        }
        let kind = match send.opcode {
            OpCode::Add | OpCode::Add_CvTmp | OpCode::Add_TmpTmp => {
                crate::vm::function::ScalarDoubleOpKind::Add
            }
            OpCode::Sub | OpCode::Sub_CvConst | OpCode::Sub_TmpTmp => {
                crate::vm::function::ScalarDoubleOpKind::Subtract
            }
            OpCode::Mul => crate::vm::function::ScalarDoubleOpKind::Multiply,
            OpCode::Div => crate::vm::function::ScalarDoubleOpKind::Divide,
            _ => return None,
        };
        if expression_count == 8
            || !matches!(send.result_type, OpType::Tmp | OpType::Var)
            || produced_temporary_slots[..expression_count].contains(&send.result)
        {
            return None;
        }
        let lhs = quick_double_argument_source(
            op_array,
            send.op1_type,
            send.op1,
            induction_cv,
            &produced_temporary_slots,
            expression_count,
            &mut input_slots,
            &mut input_count,
            &mut double_input_mask,
            total_slots,
        )?;
        let rhs = quick_double_argument_source(
            op_array,
            send.op2_type,
            send.op2,
            induction_cv,
            &produced_temporary_slots,
            expression_count,
            &mut input_slots,
            &mut input_count,
            &mut double_input_mask,
            total_slots,
        )?;
        if !lhs.is_double && !rhs.is_double {
            return None;
        }
        operations.push(QuickDoubleArgumentOp {
            kind,
            lhs: lhs.source,
            rhs: rhs.source,
        });
        produced_temporary_slots[expression_count] = send.result;
        expression_count += 1;
        cursor += 1;
    }

    let do_fcall_ip = cursor;
    let do_fcall = *op_array.instructions.get(do_fcall_ip)?;
    let sum_ip = do_fcall_ip + 1;
    let sum = *op_array.instructions.get(sum_ip)?;
    let assign = *op_array.instructions.get(sum_ip + 1)?;
    let increment_ip = sum_ip + 2;
    let increment = *op_array.instructions.get(increment_ip)?;
    if increment_ip + 1 != backedge_ip
        || do_fcall.opcode != OpCode::DoFcall
        || do_fcall.result_type != OpType::Tmp
        || sum.opcode != OpCode::Add_CvTmp
        || sum.op1_type != OpType::Cv
        || sum.op2_type != OpType::Tmp
        || sum.op2 != do_fcall.result
        || sum.result_type != OpType::Tmp
        || assign.opcode != OpCode::AssignCv
        || assign.op1_type != OpType::Cv
        || assign.op1 != sum.op1
        || assign.op2_type != OpType::Tmp
        || assign.op2 != sum.result
        || increment.op1_type != OpType::Cv
        || increment.op1 != induction_cv
        || !matches!(increment.opcode, OpCode::PreInc | OpCode::PostInc)
        || !matches!(backedge.opcode, OpCode::Jmp | OpCode::QuickLongLoopJmp)
        || backedge.op1 as usize != header_ip
    {
        return None;
    }
    let accumulator_cv = sum.op1;
    let increment_kind = if increment.opcode == OpCode::PreInc {
        QuickIncrementKind::Pre
    } else {
        QuickIncrementKind::Post
    };
    let increment_tmp = match increment.result_type {
        OpType::Unused => None,
        OpType::Tmp => Some(increment.result),
        _ => return None,
    };
    if accumulator_cv == induction_cv
        || matches!(bound, QuickLongBound::Cv(slot) if slot == induction_cv || slot == accumulator_cv)
        || double_input_mask & ((1u64 << induction_cv) | (1u64 << accumulator_cv)) != 0
        || matches!(bound, QuickLongBound::Cv(slot) if double_input_mask & (1u64 << slot) != 0)
        || receiver_slot.is_some_and(|slot| {
            slot == induction_cv
                || slot == accumulator_cv
                || slot as u32 >= op_array.num_cvs
                || double_input_mask & (1u64 << slot) != 0
                || matches!(bound, QuickLongBound::Cv(bound) if bound == slot)
        })
        || induction_cv as u32 >= op_array.num_cvs
        || accumulator_cv as u32 >= op_array.num_cvs
        || matches!(bound, QuickLongBound::Cv(slot) if slot as u32 >= op_array.num_cvs)
    {
        return None;
    }
    if let Some(source) = typed_invariant_source.as_ref() {
        let input_slot = match source.producer {
            QuickTypedInvariantProducer::JsonDecodeAssociative {
                input: QuickInvariantInput::StringSlot(slot),
            } => Some(slot),
            QuickTypedInvariantProducer::JsonDecodeAssociative {
                input: QuickInvariantInput::StringLiteral(_),
            } => None,
        };
        if source.projections.is_empty()
            || json_fetch_mask
                & !(json_parent_mask | source.double_output_mask)
                != 0
            || source.destination == induction_cv
            || source.destination == accumulator_cv
            || matches!(bound, QuickLongBound::Cv(slot) if source.destination == slot)
            || receiver_slot == Some(source.destination)
            || input_slot.is_some_and(|slot| {
                slot == induction_cv
                    || slot == accumulator_cv
                    || matches!(bound, QuickLongBound::Cv(bound) if bound == slot)
                    || receiver_slot == Some(slot)
                    || double_input_mask & (1u64 << slot) != 0
            })
        {
            return None;
        }
    }
    let mut temporary_mask = 0u64;
    for slot in [
        condition_tmp,
        Some(do_fcall.result),
        Some(sum.result),
        increment_tmp,
    ]
    .into_iter()
    .flatten()
    {
        add_mask_slot(&mut temporary_mask, slot, total_slots)?;
    }
    for slot in produced_temporary_slots
        .iter()
        .copied()
        .take(expression_count)
    {
        add_mask_slot(&mut temporary_mask, slot, total_slots)?;
    }
    let mut json_temporaries = json_fetch_mask;
    while json_temporaries != 0 {
        let slot = json_temporaries.trailing_zeros() as u16;
        json_temporaries &= json_temporaries - 1;
        add_mask_slot(&mut temporary_mask, slot, total_slots)?;
    }
    let expected_temporary_count = usize::from(condition_tmp.is_some())
        + 2
        + usize::from(increment_tmp.is_some())
        + expression_count
        + json_fetch_mask.count_ones() as usize;
    if temporary_mask.count_ones() as usize != expected_temporary_count {
        return None;
    }

    Some(QuickDoubleCallAccumulateLoop {
        header_ip,
        exit_ip,
        induction_cv,
        bound,
        accumulator_cv,
        condition_tmp,
        guard,
        argument_program: QuickDoubleArgumentProgram {
            operations: operations.into_boxed_slice(),
            outputs,
            output_count: argument_count as u8,
            input_slots,
            input_count: input_count as u8,
        },
        typed_invariant_source,
        term_tmp: do_fcall.result,
        sum_tmp: sum.result,
        increment_kind,
        increment_tmp,
        sum_ip,
        increment_ip,
        #[cfg(all(
            feature = "jit-prototype",
            any(
                all(target_arch = "aarch64", target_os = "macos"),
                all(target_arch = "x86_64", target_os = "linux")
            )
        ))]
        native_jit: crate::jit::QuickDoubleCallAccumulateJitCache::new(),
    })
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
        || body.select.is_some()
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

fn detect_scalar_call_tree(
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
            let nested_do_fcall_ip = detect_scalar_call_tree(
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
    let call_ip = header_ip + 2;
    let nested_function_call_shape = if first_body.opcode == OpCode::InitFcall {
        let total_slots = op_array.num_cvs.checked_add(op_array.num_temps)?;
        if total_slots > 64 {
            return None;
        }
        let mut long_input_mask = 0u64;
        let mut object_input_mask = 0u64;
        let do_fcall_ip = detect_scalar_call_tree(
            op_array,
            call_ip,
            total_slots,
            &mut long_input_mask,
            &mut object_input_mask,
            0,
        )?;
        let contains_nested_call = op_array.instructions[call_ip + 1..do_fcall_ip]
            .iter()
            .any(|instruction| {
                matches!(instruction.opcode, OpCode::InitFcall | OpCode::InitMethodCall)
            });
        if contains_nested_call {
            if long_input_mask & object_input_mask != 0 {
                return None;
            }
            let argument_count = u8::try_from(first_body.op1).ok()?;
            let do_fcall = op_array.instructions[do_fcall_ip];
            let sum_ip = do_fcall_ip + 1;
            let sum = *op_array.instructions.get(sum_ip)?;
            if backedge_ip < do_fcall_ip + 4
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
                QuickLongTerm::ScalarCallTree {
                    guard: ScalarLongCallGuard::FunctionCache {
                        cache_ip: u32::try_from(call_ip).ok()?,
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
        }
    } else {
        None
    };
    let scalar_call_shape = if nested_function_call_shape.is_some() {
        nested_function_call_shape
    } else if first_body.opcode == OpCode::InitFcall {
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
        if backedge_ip < do_fcall_ip + 4
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
        let do_fcall_ip = detect_scalar_call_tree(
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
        if backedge_ip < do_fcall_ip + 4
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
            QuickLongTerm::ScalarCallTree {
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
    } else if first_body.opcode == OpCode::Add
        && first_body.op1_type == OpType::Cv
        && first_body.op2_type == OpType::Cv
        && first_body.op2 == induction_cv
        && first_body.result_type == OpType::Tmp
        && op_array
            .instructions
            .get(header_ip + 3)
            .is_some_and(|assign| {
                assign.opcode == OpCode::AssignCv
                    && assign.op1_type == OpType::Cv
                    && assign.op1 == first_body.op1
                    && assign.op2_type == OpType::Tmp
                    && assign.op2 == first_body.result
                    && assign.result_type == OpType::Unused
            })
    {
        if backedge_ip < header_ip + 5 {
            return None;
        }
        (
            first_body.op1,
            QuickLongTerm::Induction,
            first_body.result,
            header_ip + 2,
            header_ip + 3,
        )
    } else if backedge_ip >= header_ip + 6
        && op_array
            .instructions
            .get(header_ip + 3)
            .is_some_and(|sum| {
                sum.opcode == OpCode::Add_CvTmp
                    && sum.op1_type == OpType::Cv
                    && sum.op2_type == OpType::Tmp
                    && sum.op2 == first_body.result
                    && sum.result_type == OpType::Tmp
            })
    {
        let sum = op_array.instructions[header_ip + 3];
        if first_body.result_type != OpType::Tmp {
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

    let total_slots = op_array.num_cvs.checked_add(op_array.num_temps)?;
    let direct_increment_ip = assign_ip + 1;
    let (tail_guard, increment_ip) = if matches!(
        op_array.instructions.get(direct_increment_ip)?.opcode,
        OpCode::PreInc | OpCode::PostInc
    ) {
        (None, direct_increment_ip)
    } else {
        let increment_ip = backedge_ip.checked_sub(1)?;
        (
            Some(detect_long_tail_trace_guard(
                op_array,
                direct_increment_ip,
                increment_ip,
                total_slots,
            )?),
            increment_ip,
        )
    };
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
        | QuickLongTerm::ScalarCallTree { term_tmp, .. } => {
            temporary_slots.push(term_tmp);
        }
    }
    if let Some(slot) = increment_tmp {
        temporary_slots.push(slot);
    }
    if let Some(slot) = tail_guard.and_then(|guard| guard.condition_tmp) {
        temporary_slots.push(slot);
    }
    let temporary_slot_count = temporary_slots.len();
    temporary_slots.sort_unstable();
    temporary_slots.dedup();
    if temporary_slots.len() != temporary_slot_count {
        return None;
    }

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
        tail_guard,
        increment_kind,
        increment_tmp,
        sum_ip,
        increment_ip,
        #[cfg(all(
            feature = "jit-prototype",
            any(
                all(target_arch = "aarch64", target_os = "macos"),
                all(target_arch = "x86_64", target_os = "linux")
            )
        ))]
        native_jit: crate::jit::QuickLongAccumulateJitCache::new(),
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

fn quick_long_operand(
    op_array: &OpArray,
    op_type: OpType,
    operand: u16,
) -> Option<QuickLongOperand> {
    match op_type {
        OpType::Cv | OpType::Tmp => Some(QuickLongOperand::Slot(operand)),
        OpType::Const => Some(QuickLongOperand::Const(long_literal(op_array, operand)?)),
        _ => None,
    }
}

fn detect_long_tail_trace_guard(
    op_array: &OpArray,
    guard_ip: usize,
    increment_ip: usize,
    total_slots: u32,
) -> Option<QuickLongTraceGuard> {
    // The conditional jump must skip at least one cold instruction and land
    // directly on the loop increment. The cold range itself stays canonical
    // and may contain arbitrary PHP behavior.
    if guard_ip.checked_add(2)? >= increment_ip {
        return None;
    }
    let comparison = *op_array.instructions.get(guard_ip)?;
    let kind = match comparison.opcode {
        OpCode::IsIdentical => ScalarLongConditionKind::Equal,
        OpCode::IsNotIdentical => ScalarLongConditionKind::NotEqual,
        _ => return None,
    };
    let operand = |op_type, value| match op_type {
        OpType::Cv => {
            (u32::from(value) < op_array.num_cvs).then_some(QuickLongOperand::Slot(value))
        }
        OpType::Const => Some(QuickLongOperand::Const(long_literal(op_array, value)?)),
        _ => None,
    };
    let lhs = operand(comparison.op1_type, comparison.op1)?;
    let rhs = operand(comparison.op2_type, comparison.op2)?;
    if comparison.result_type != OpType::Tmp
        || u32::from(comparison.result) >= total_slots
    {
        return None;
    }
    let branch = *op_array.instructions.get(guard_ip + 1)?;
    if branch.op1_type != OpType::Tmp
        || branch.op1 != comparison.result
        || branch.op2_type != OpType::Unused
        || branch.op2 as usize != increment_ip
    {
        return None;
    }
    let expected = match branch.opcode {
        OpCode::JmpZ => false,
        OpCode::JmpNZ => true,
        _ => return None,
    };
    Some(QuickLongTraceGuard {
        kind,
        lhs,
        rhs,
        expected,
        condition_tmp: Some(comparison.result),
        resume_ip: guard_ip,
    })
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

/// Recognize the shared loop-invariant associative JSON producer. Consumers
/// own path/use validation, but input stability and the canonical materialized
/// destination are identical for Long, Double and String regions.
fn detect_json_typed_invariant_source(
    op_array: &OpArray,
    region: &[crate::vm::instruction::Instruction],
    producer_ip: usize,
) -> Option<QuickTypedInvariantSource> {
    let producer = *op_array.instructions.get(producer_ip)?;
    if producer.opcode != OpCode::DirectInternalCall2
        || crate::builtin_metadata::DirectInternalKind::from_id(producer.extended_value)
            != Some(crate::builtin_metadata::DirectInternalKind::JsonDecode)
        || !matches!(producer.op1_type, OpType::Cv | OpType::Const)
        || producer.op2_type != OpType::Const
        || !matches!(producer.result_type, OpType::Tmp | OpType::Var)
        || op_array
            .literals
            .get(producer.op2 as usize)?
            .value_type()
            != crate::value::ValueType::True
    {
        return None;
    }
    let input = match producer.op1_type {
        OpType::Cv if cv_unmodified_in_region(region, producer.op1) => {
            QuickInvariantInput::StringSlot(producer.op1)
        }
        OpType::Const => {
            op_array
                .literals
                .get(producer.op1 as usize)?
                .as_str()?;
            QuickInvariantInput::StringLiteral(producer.op1)
        }
        _ => return None,
    };
    let assignment = *op_array.instructions.get(producer_ip + 1)?;
    if assignment.opcode != OpCode::AssignCv
        || assignment.op1_type != OpType::Cv
        || assignment.op2_type != producer.result_type
        || assignment.op2 != producer.result
        || assignment.result_type != OpType::Unused
    {
        return None;
    }
    Some(QuickTypedInvariantSource {
        producer: QuickTypedInvariantProducer::JsonDecodeAssociative { input },
        destination: assignment.op1,
        projections: Vec::new(),
        long_output_mask: 0,
        double_output_mask: 0,
        string_output_mask: 0,
    })
}

fn fixed_invariant_path_element(
    op_array: &OpArray,
    op_type: OpType,
    operand: u16,
) -> Option<QuickInvariantPathElement> {
    if op_type != OpType::Const {
        return None;
    }
    let value = op_array.literals.get(operand as usize)?;
    Some(if value.as_str().is_some() {
        QuickInvariantPathElement::StringLiteral(operand)
    } else {
        QuickInvariantPathElement::Integer(value.as_long()?)
    })
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

/// Determine the Long values that must exist before a straight-line region
/// starts. Temporaries produced earlier in the same region are deliberately
/// excluded, so a region can activate in a fresh function frame rather than
/// depending on stale temporary-slot contents from an earlier execution.
fn straight_region_read(inputs: &mut u64, defined: u64, slot: u16) -> Option<()> {
    let bit = 1u64.checked_shl(u32::from(slot))?;
    if defined & bit == 0 {
        *inputs |= bit;
    }
    Some(())
}

fn straight_region_write(defined: &mut u64, slot: u16) -> Option<()> {
    *defined |= 1u64.checked_shl(u32::from(slot))?;
    Some(())
}

fn straight_region_read_operand(
    inputs: &mut u64,
    defined: u64,
    operand: QuickLongOperand,
) -> Option<()> {
    match operand {
        QuickLongOperand::Slot(slot) => straight_region_read(inputs, defined, slot),
        QuickLongOperand::Const(_) => Some(()),
    }
}

fn straight_long_region_inputs(ops: &[QuickLongOp]) -> Option<u64> {
    let mut inputs = 0u64;
    let mut defined = 0u64;

    for op in ops {
        match *op {
            QuickLongOp::ModConst { value, result, .. } => {
                straight_region_read(&mut inputs, defined, value)?;
                straight_region_write(&mut defined, result)?;
            }
            QuickLongOp::TraceGuard {
                lhs,
                rhs,
                condition_tmp,
                ..
            } => {
                straight_region_read_operand(&mut inputs, defined, lhs)?;
                straight_region_read_operand(&mut inputs, defined, rhs)?;
                if let Some(condition_tmp) = condition_tmp {
                    straight_region_write(&mut defined, condition_tmp)?;
                }
            }
            QuickLongOp::Binary {
                lhs, rhs, result, ..
            } => {
                straight_region_read_operand(&mut inputs, defined, lhs)?;
                straight_region_read_operand(&mut inputs, defined, rhs)?;
                straight_region_write(&mut defined, result)?;
            }
            QuickLongOp::BinaryAssign {
                lhs,
                rhs,
                result,
                destination,
                ..
            } => {
                straight_region_read_operand(&mut inputs, defined, lhs)?;
                straight_region_read_operand(&mut inputs, defined, rhs)?;
                straight_region_write(&mut defined, result)?;
                straight_region_write(&mut defined, destination)?;
            }
            QuickLongOp::FetchArrayLong {
                index,
                result,
                destination,
                ..
            } => {
                if let QuickArrayIndex::Long(operand) = index {
                    straight_region_read_operand(&mut inputs, defined, operand)?;
                }
                straight_region_write(&mut defined, result)?;
                if let Some(destination) = destination {
                    straight_region_write(&mut defined, destination)?;
                }
            }
            QuickLongOp::Add {
                lhs, rhs, result, ..
            } => {
                straight_region_read(&mut inputs, defined, lhs)?;
                straight_region_read(&mut inputs, defined, rhs)?;
                straight_region_write(&mut defined, result)?;
            }
            QuickLongOp::AddAssign {
                lhs,
                rhs,
                result,
                destination,
                ..
            } => {
                straight_region_read(&mut inputs, defined, lhs)?;
                straight_region_read(&mut inputs, defined, rhs)?;
                straight_region_write(&mut defined, result)?;
                straight_region_write(&mut defined, destination)?;
            }
            QuickLongOp::ConditionalAddAssign {
                condition,
                condition_tmp,
                lhs,
                rhs,
                result,
                destination,
                ..
            } => {
                match condition {
                    QuickLongCondition::Lt { lhs, rhs }
                    | QuickLongCondition::Eq { lhs, rhs } => {
                        straight_region_read(&mut inputs, defined, lhs)?;
                        straight_region_read_operand(&mut inputs, defined, rhs)?;
                    }
                }
                if let Some(condition_tmp) = condition_tmp {
                    straight_region_write(&mut defined, condition_tmp)?;
                }
                straight_region_read(&mut inputs, defined, lhs)?;
                straight_region_read(&mut inputs, defined, rhs)?;
                straight_region_write(&mut defined, result)?;
                straight_region_write(&mut defined, destination)?;
            }
            QuickLongOp::AddAddAssign {
                first_lhs,
                first_rhs,
                first_result,
                second_lhs,
                second_rhs,
                second_result,
                destination,
                ..
            } => {
                straight_region_read(&mut inputs, defined, first_lhs)?;
                straight_region_read(&mut inputs, defined, first_rhs)?;
                straight_region_write(&mut defined, first_result)?;
                straight_region_read(&mut inputs, defined, second_lhs)?;
                straight_region_read(&mut inputs, defined, second_rhs)?;
                straight_region_write(&mut defined, second_result)?;
                straight_region_write(&mut defined, destination)?;
            }
            QuickLongOp::Assign {
                destination,
                source,
                ..
            } => {
                straight_region_read(&mut inputs, defined, source)?;
                straight_region_write(&mut defined, destination)?;
            }
            QuickLongOp::AssignLongLiteral { destination, .. } => {
                straight_region_write(&mut defined, destination)?;
            }
            QuickLongOp::PostInc {
                value, result, ..
            } => {
                straight_region_read(&mut inputs, defined, value)?;
                if let Some(result) = result {
                    straight_region_write(&mut defined, result)?;
                }
                straight_region_write(&mut defined, value)?;
            }
            // The first application-region slice is intentionally bounded by
            // calls, observable mutation, and control-flow edges. Closed loops
            // continue to use the complete operation vocabulary.
            _ => return None,
        }
    }
    Some(inputs)
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
    detect_long_ops_region_inner(op_array, header_ip, backedge_ip, true)
}

/// Build a typed, straight-line application region between semantic events.
/// Calls, returns, visible mutation, and control-flow edges remain baseline
/// boundaries; the returned plan shares the exact side-exit contract and
/// executor with closed quick loops.
pub fn detect_long_ops_region(
    op_array: &OpArray,
    entry_ip: usize,
    end_ip: usize,
) -> Option<QuickLongOpsLoop> {
    detect_long_ops_region_inner(op_array, entry_ip, end_ip, false)
}

fn detect_long_ops_region_inner(
    op_array: &OpArray,
    header_ip: usize,
    backedge_ip: usize,
    closed_loop: bool,
) -> Option<QuickLongOpsLoop> {
    if header_ip >= backedge_ip
        || backedge_ip >= op_array.instructions.len()
        || backedge_ip - header_ip >= u16::MAX as usize
    {
        return None;
    }

    if closed_loop {
        let backedge = op_array.instructions[backedge_ip];
        if !matches!(backedge.opcode, OpCode::Jmp | OpCode::QuickLongLoopJmp)
            || backedge.op1 as usize != header_ip
        {
            return None;
        }
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
    let mut array_output_mask = 0u64;
    let mut string_input_mask = 0u64;
    let mut string_output_mask = 0u64;
    let mut string_append_mask = 0u64;
    let mut object_input_mask = 0u64;
    let mut typed_invariant_source = None;
    let mut json_paths: Vec<Option<Vec<QuickInvariantPathElement>>> =
        vec![None; total_slots as usize];
    let mut json_fetch_mask = 0u64;
    let mut json_parent_mask = 0u64;
    let mut json_string_source_mask = 0u64;
    let mut json_string_length_paths: Vec<Option<Vec<QuickInvariantPathElement>>> =
        vec![None; total_slots as usize];
    let mut has_add = false;
    let mut has_assign = false;
    let mut has_object_call = false;
    let mut has_array_push = false;
    let mut has_string_append = false;
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

    // A non-escaping virtual constructor may also retain a string CV even
    // though it is never used as an array key in the caller. Admit only CVs
    // whose complete visible assignment set below proves immutable strings.
    let mut virtual_string_candidate_mask = 0u64;
    for (relative_ip, instruction) in region.iter().copied().enumerate() {
        if instruction.opcode != OpCode::NewObj
            || instruction._pad
                & crate::vm::instruction::NEW_FLAG_VIRTUAL_OBJECT_ARRAY_PIPELINE
                == 0
        {
            continue;
        }
        let new_ip = header_ip + relative_ip;
        for index in 0..instruction.extended_value as usize {
            let send = *op_array.instructions.get(new_ip + 1 + index)?;
            if send.op1_type == OpType::Cv {
                add_mask_slot(
                    &mut virtual_string_candidate_mask,
                    send.op1,
                    total_slots,
                )?;
            }
        }
    }

    let mut string_key_assignment_mask = 0u64;
    let mut candidates = array_index_cv_mask | virtual_string_candidate_mask;
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
            OpCode::DirectInternalCall2
                if crate::builtin_metadata::DirectInternalKind::from_id(
                    instruction.extended_value,
                ) == Some(crate::builtin_metadata::DirectInternalKind::JsonDecode) =>
            {
                let skipped_by_prior_edge = ops.iter().skip(1).any(|operation| {
                    match *operation {
                        QuickLongOp::BranchUnlessLt { false_target, .. }
                        | QuickLongOp::BranchUnlessEq { false_target, .. }
                        | QuickLongOp::BranchUnlessLe { false_target, .. } => false_target
                            .unresolved_ip()
                            .is_some_and(|target| target > ip),
                        QuickLongOp::TraceGuard { .. } | QuickLongOp::Jump { .. } => true,
                        _ => false,
                    }
                });
                if typed_invariant_source.is_some()
                    || skipped_by_prior_edge
                {
                    return None;
                }
                let source = detect_json_typed_invariant_source(op_array, region, ip)?;
                if let QuickTypedInvariantProducer::JsonDecodeAssociative {
                    input: QuickInvariantInput::StringSlot(slot),
                } = source.producer
                {
                    add_mask_slot(&mut string_input_mask, slot, total_slots)?;
                }
                json_paths
                    .get_mut(source.destination as usize)?
                    .replace(Vec::new());
                typed_invariant_source = Some(source);
                has_assign = true;
                let resume_ip = ip;
                ip += 2;
                QuickLongOp::JsonProjectionStep {
                    next_target: QuickLongTarget::unresolved(ip)?,
                    resume_ip,
                }
            }
            OpCode::FetchDimR => {
                let array = long_slot(instruction.op1_type, instruction.op1)?;
                if instruction.result_type != OpType::Tmp {
                    return None;
                }
                if let Some(mut path) = json_paths
                    .get(array as usize)
                    .and_then(|path| path.as_ref())
                    .cloned()
                {
                    let element = fixed_invariant_path_element(
                        op_array,
                        instruction.op2_type,
                        instruction.op2,
                    )?;
                    if path.len() == 8 {
                        return None;
                    }
                    path.push(element);
                    json_paths
                        .get_mut(instruction.result as usize)?
                        .replace(path);
                    add_mask_slot(
                        &mut json_fetch_mask,
                        instruction.result,
                        total_slots,
                    )?;
                    add_mask_slot(&mut json_parent_mask, array, total_slots)?;
                    let resume_ip = ip;
                    ip += 1;
                    QuickLongOp::JsonProjectionStep {
                        next_target: QuickLongTarget::unresolved(ip)?,
                        resume_ip,
                    }
                } else {
                    let index = match instruction.op2_type {
                        OpType::Cv | OpType::Tmp => {
                            if instruction.op2_type == OpType::Cv
                                && string_key_assignment_mask & (1u64 << instruction.op2) != 0
                            {
                                add_mask_slot(
                                    &mut string_input_mask,
                                    instruction.op2,
                                    total_slots,
                                )?;
                                QuickArrayIndex::ValueSlot(instruction.op2)
                            } else {
                                add_mask_slot(
                                    &mut long_input_mask,
                                    instruction.op2,
                                    total_slots,
                                )?;
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
            }
            OpCode::Strlen | OpCode::Strlen_String => {
                if !matches!(instruction.op1_type, OpType::Tmp | OpType::Var)
                    || !matches!(instruction.result_type, OpType::Tmp | OpType::Var)
                {
                    return None;
                }
                let Some(path) = json_paths
                    .get(instruction.op1 as usize)
                    .and_then(|path| path.as_ref())
                    .cloned()
                else {
                    return None;
                };
                if path.is_empty()
                    || json_string_length_paths
                        .get(instruction.result as usize)?
                        .is_some()
                {
                    return None;
                }
                add_mask_slot(
                    &mut json_string_source_mask,
                    instruction.op1,
                    total_slots,
                )?;
                add_mask_slot(&mut long_input_mask, instruction.result, total_slots)?;
                json_string_length_paths
                    .get_mut(instruction.result as usize)?
                    .replace(path);
                let resume_ip = ip;
                ip += 1;
                QuickLongOp::JsonProjectionStep {
                    next_target: QuickLongTarget::unresolved(ip)?,
                    resume_ip,
                }
            }
            OpCode::AssignDim => {
                if json_paths
                    .get(instruction.op1 as usize)
                    .and_then(|path| path.as_ref())
                    .is_some()
                {
                    // Reusing one decoded array is observable once the loop
                    // mutates it; canonical execution creates a fresh array on
                    // every iteration. Keep all such roots on the baseline.
                    return None;
                }
                if instruction.op1_type != OpType::Cv
                    || !matches!(instruction.result_type, OpType::Cv | OpType::Tmp | OpType::Var)
                {
                    return None;
                }
                let array = instruction.op1;
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

                // Updating through a retained raw array view is safe only when
                // a preceding fetch proved that this exact key already exists;
                // replacement then cannot resize or reorder the array.
                let [.., fetch, arithmetic] = ops.as_slice() else {
                    return None;
                };
                let (fetch_array, fetch_index, fetch_result) = match *fetch {
                    QuickLongOp::FetchArrayLong {
                        array,
                        index,
                        result,
                        ..
                    } => (array, index, result),
                    _ => return None,
                };
                let (arithmetic_result, consumes_fetch) = match *arithmetic {
                    QuickLongOp::Add { lhs, rhs, result, .. } => {
                        (result, lhs == fetch_result || rhs == fetch_result)
                    }
                    QuickLongOp::Binary { lhs, rhs, result, .. } => (
                        result,
                        lhs == QuickLongOperand::Slot(fetch_result)
                            || rhs == QuickLongOperand::Slot(fetch_result),
                    ),
                    _ => return None,
                };
                if fetch_array != array
                    || fetch_index != index
                    || arithmetic_result != instruction.result
                    || !consumes_fetch
                {
                    return None;
                }

                add_mask_slot(&mut array_input_mask, array, total_slots)?;
                add_mask_slot(&mut array_output_mask, array, total_slots)?;
                add_mask_slot(&mut long_input_mask, instruction.result, total_slots)?;
                has_assign = true;
                let resume_ip = ip;
                ip += 1;
                QuickLongOp::StoreArrayLong {
                    array,
                    index,
                    value: instruction.result,
                    next_target: QuickLongTarget::unresolved(ip)?,
                    resume_ip,
                }
            }
            OpCode::ArrayPushOp => {
                if json_paths
                    .get(instruction.op1 as usize)
                    .and_then(|path| path.as_ref())
                    .is_some()
                {
                    return None;
                }
                if instruction.op1_type != OpType::Cv
                    || instruction.result_type != OpType::Unused
                {
                    return None;
                }
                let value = match instruction.op2_type {
                    OpType::Cv | OpType::Tmp | OpType::Var => {
                        add_mask_slot(&mut long_input_mask, instruction.op2, total_slots)?;
                        QuickLongOperand::Slot(instruction.op2)
                    }
                    OpType::Const => {
                        QuickLongOperand::Const(long_literal(op_array, instruction.op2)?)
                    }
                    OpType::Unused => return None,
                };
                add_mask_slot(&mut array_output_mask, instruction.op1, total_slots)?;
                has_array_push = true;
                let resume_ip = ip;
                ip += 1;
                QuickLongOp::ArrayPushLong {
                    array: instruction.op1,
                    value,
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
            OpCode::IsSmallerOrEqual => {
                let lhs = quick_long_operand(
                    op_array,
                    instruction.op1_type,
                    instruction.op1,
                )?;
                let rhs = quick_long_operand(
                    op_array,
                    instruction.op2_type,
                    instruction.op2,
                )?;
                let branch = *op_array.instructions.get(ip + 1)?;
                if instruction.result_type != OpType::Tmp
                    || branch.opcode != OpCode::JmpZ
                    || branch.op1_type != OpType::Tmp
                    || branch.op1 != instruction.result
                    || branch.op2_type != OpType::Unused
                {
                    return None;
                }
                for operand in [lhs, rhs] {
                    if let QuickLongOperand::Slot(slot) = operand {
                        add_mask_slot(&mut long_input_mask, slot, total_slots)?;
                    }
                }
                add_mask_slot(&mut bool_output_mask, instruction.result, total_slots)?;
                let op = QuickLongOp::BranchUnlessLe {
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
            OpCode::IsIdentical | OpCode::IsNotIdentical => {
                if !closed_loop {
                    return None;
                }
                let lhs = quick_long_operand(
                    op_array,
                    instruction.op1_type,
                    instruction.op1,
                )?;
                let rhs = quick_long_operand(
                    op_array,
                    instruction.op2_type,
                    instruction.op2,
                )?;
                let branch = *op_array.instructions.get(ip + 1)?;
                if instruction.result_type != OpType::Tmp
                    || branch.op1_type != OpType::Tmp
                    || branch.op1 != instruction.result
                    || branch.op2_type != OpType::Unused
                {
                    return None;
                }
                let expected = match branch.opcode {
                    OpCode::JmpZ => false,
                    OpCode::JmpNZ => true,
                    _ => return None,
                };
                let target_ip = branch.op2 as usize;
                // The selected edge must skip at least one cold instruction
                // and rejoin this closed loop before its baseline backedge.
                if target_ip <= ip + 2 || target_ip >= backedge_ip {
                    return None;
                }
                for operand in [lhs, rhs] {
                    if let QuickLongOperand::Slot(slot) = operand {
                        add_mask_slot(&mut long_input_mask, slot, total_slots)?;
                    }
                }
                add_mask_slot(
                    &mut bool_output_mask,
                    instruction.result,
                    total_slots,
                )?;
                let resume_ip = ip;
                ip = target_ip;
                QuickLongOp::TraceGuard {
                    kind: match instruction.opcode {
                        OpCode::IsIdentical => ScalarLongConditionKind::Equal,
                        OpCode::IsNotIdentical => ScalarLongConditionKind::NotEqual,
                        _ => unreachable!(),
                    },
                    lhs,
                    rhs,
                    expected,
                    condition_tmp: Some(instruction.result),
                    next_target: QuickLongTarget::unresolved(target_ip)?,
                    resume_ip,
                }
            }
            OpCode::Add
                if long_slot(instruction.op1_type, instruction.op1).is_none()
                    || long_slot(instruction.op2_type, instruction.op2).is_none() =>
            {
                if instruction.result_type != OpType::Tmp {
                    return None;
                }
                let lhs = quick_long_operand(
                    op_array,
                    instruction.op1_type,
                    instruction.op1,
                )?;
                let rhs = quick_long_operand(
                    op_array,
                    instruction.op2_type,
                    instruction.op2,
                )?;
                for operand in [lhs, rhs] {
                    if let QuickLongOperand::Slot(slot) = operand {
                        add_mask_slot(&mut long_input_mask, slot, total_slots)?;
                    }
                }
                add_mask_slot(&mut long_output_mask, instruction.result, total_slots)?;
                has_add = true;
                let resume_ip = ip;
                let destination = op_array
                    .instructions
                    .get(ip + 1)
                    .copied()
                    .and_then(long_assign)
                    .and_then(|(destination, source)| {
                        (source == instruction.result).then_some(destination)
                    });
                if let Some(destination) = destination {
                    add_mask_slot(&mut long_output_mask, destination, total_slots)?;
                    has_assign = true;
                    ip += 2;
                    QuickLongOp::BinaryAssign {
                        kind: ScalarLongOpKind::Add,
                        lhs,
                        rhs,
                        result: instruction.result,
                        destination,
                        next_target: QuickLongTarget::unresolved(ip)?,
                        resume_ip,
                    }
                } else {
                    ip += 1;
                    QuickLongOp::Binary {
                        kind: ScalarLongOpKind::Add,
                        lhs,
                        rhs,
                        result: instruction.result,
                        next_target: QuickLongTarget::unresolved(ip)?,
                        resume_ip,
                    }
                }
            }
            OpCode::Sub
            | OpCode::Sub_CvConst
            | OpCode::Sub_TmpTmp
            | OpCode::Mul
            | OpCode::BitwiseAnd
            | OpCode::BitwiseOr
            | OpCode::BitwiseXor
            | OpCode::BitwiseXor_LongLong => {
                if instruction.result_type != OpType::Tmp {
                    return None;
                }
                let lhs = quick_long_operand(
                    op_array,
                    instruction.op1_type,
                    instruction.op1,
                )?;
                let rhs = quick_long_operand(
                    op_array,
                    instruction.op2_type,
                    instruction.op2,
                )?;
                for operand in [lhs, rhs] {
                    if let QuickLongOperand::Slot(slot) = operand {
                        add_mask_slot(&mut long_input_mask, slot, total_slots)?;
                    }
                }
                add_mask_slot(&mut long_output_mask, instruction.result, total_slots)?;
                let kind = match instruction.opcode {
                    OpCode::Mul => ScalarLongOpKind::Multiply,
                    OpCode::BitwiseAnd => ScalarLongOpKind::BitwiseAnd,
                    OpCode::BitwiseOr => ScalarLongOpKind::BitwiseOr,
                    OpCode::BitwiseXor | OpCode::BitwiseXor_LongLong => {
                        ScalarLongOpKind::BitwiseXor
                    }
                    _ => ScalarLongOpKind::Subtract,
                };
                let resume_ip = ip;
                let destination = op_array
                    .instructions
                    .get(ip + 1)
                    .copied()
                    .and_then(long_assign)
                    .and_then(|(destination, source)| {
                        (source == instruction.result).then_some(destination)
                    });
                if let Some(destination) = destination {
                    add_mask_slot(&mut long_output_mask, destination, total_slots)?;
                    has_assign = true;
                    ip += 2;
                    QuickLongOp::BinaryAssign {
                        kind,
                        lhs,
                        rhs,
                        result: instruction.result,
                        destination,
                        next_target: QuickLongTarget::unresolved(ip)?,
                        resume_ip,
                    }
                } else {
                    ip += 1;
                    QuickLongOp::Binary {
                        kind,
                        lhs,
                        rhs,
                        result: instruction.result,
                        next_target: QuickLongTarget::unresolved(ip)?,
                        resume_ip,
                    }
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
            OpCode::NewObj => {
                if instruction._pad
                    & crate::vm::instruction::NEW_FLAG_VIRTUAL_OBJECT_ARRAY_PIPELINE
                    == 0
                {
                    return None;
                }
                let next_ip = detect_virtual_object_array_pipeline_span(op_array, ip)?;
                if next_ip <= ip || next_ip > backedge_ip {
                    return None;
                }

                let mut constructor_arguments =
                    [QuickVirtualValueSource::Long(QuickLongOperand::Const(0)); 8];
                for (index, argument) in constructor_arguments
                    .iter_mut()
                    .enumerate()
                    .take(instruction.extended_value as usize)
                {
                    let send = *op_array.instructions.get(ip + 1 + index)?;
                    *argument = match send.op1_type {
                        OpType::Cv | OpType::Tmp => {
                            let bit = 1u64.checked_shl(u32::from(send.op1))?;
                            if string_key_assignment_mask & bit != 0 {
                                add_mask_slot(
                                    &mut string_input_mask,
                                    send.op1,
                                    total_slots,
                                )?;
                                QuickVirtualValueSource::StringSlot(send.op1)
                            } else {
                                add_mask_slot(
                                    &mut long_input_mask,
                                    send.op1,
                                    total_slots,
                                )?;
                                QuickVirtualValueSource::Long(
                                    QuickLongOperand::Slot(send.op1),
                                )
                            }
                        }
                        OpType::Const => {
                            let value = op_array.literals.get(send.op1 as usize)?;
                            if let Some(value) = value.as_long() {
                                QuickVirtualValueSource::Long(
                                    QuickLongOperand::Const(value),
                                )
                            } else if value.as_str().is_some() {
                                QuickVirtualValueSource::StringLiteral(send.op1)
                            } else {
                                return None;
                            }
                        }
                        _ => return None,
                    };
                }

                let constructor_do_ip = ip + 1 + instruction.extended_value as usize;
                let method_ip = constructor_do_ip + 2;
                let method = *op_array.instructions.get(method_ip)?;
                if method.opcode != OpCode::InitMethodCall
                    || method.op1_type != OpType::Cv
                    || method.extended_value != 1
                {
                    return None;
                }
                add_mask_slot(&mut object_input_mask, method.op1, total_slots)?;

                let method_do_ip = method_ip + 1 + method.extended_value as usize;
                let mut cursor = method_do_ip + 2;
                let mut output_mask = 0u64;
                let mut consumer_count = 0usize;
                let mut consumers = [QuickObjectArrayConsumer::EMPTY; 4];
                let mut trailing_key_literal = None;
                let mut trailing_result = 0;
                while cursor < next_ip {
                    let fetch = *op_array.instructions.get(cursor)?;
                    if fetch.opcode != OpCode::FetchDimR {
                        return None;
                    }
                    let add = op_array.instructions.get(cursor + 1).copied();
                    let assign = op_array.instructions.get(cursor + 2).copied();
                    if let (Some(add), Some(assign)) = (add, assign)
                        && let Some(accumulator) =
                            object_array_add_consumer(fetch, add, assign)
                    {
                        add_mask_slot(&mut long_input_mask, accumulator, total_slots)?;
                        add_mask_slot(&mut long_output_mask, accumulator, total_slots)?;
                        output_mask |= 1u64 << accumulator;
                        *consumers.get_mut(consumer_count)? = QuickObjectArrayConsumer {
                            key_literal: fetch.op2,
                            accumulator,
                        };
                        consumer_count += 1;
                        cursor += 3;
                    } else {
                        add_mask_slot(&mut long_output_mask, fetch.result, total_slots)?;
                        output_mask |= 1u64 << fetch.result;
                        trailing_key_literal = Some(fetch.op2);
                        trailing_result = fetch.result;
                        cursor += 1;
                    }
                }
                if cursor != next_ip || consumer_count == 0 {
                    return None;
                }
                has_object_call = true;
                let resume_ip = ip;
                ip = next_ip;
                QuickLongOp::VirtualObjectArrayPipeline {
                    constructor_arguments,
                    argument_count: instruction.extended_value as u8,
                    consumers,
                    consumer_count: consumer_count as u8,
                    trailing_key_literal,
                    trailing_result,
                    output_mask,
                    next_target: QuickLongTarget::unresolved(ip)?,
                    resume_ip,
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
                    || !cv_unmodified_in_region(region, instruction.op1)
                {
                    return None;
                }
                add_mask_slot(&mut object_input_mask, instruction.op1, total_slots)?;
                has_object_call = true;
                let cache_ip = u32::try_from(ip).ok()?;
                let resume_ip = ip;
                let strlen = op_array.instructions.get(ip + 1).copied();
                if let Some(strlen) = strlen
                    && matches!(strlen.opcode, OpCode::Strlen | OpCode::Strlen_String)
                    && strlen.op1_type == instruction.result_type
                    && strlen.op1 == instruction.result
                    && matches!(strlen.result_type, OpType::Tmp | OpType::Var)
                {
                    add_mask_slot(&mut long_output_mask, strlen.result, total_slots)?;
                    ip += 2;
                    QuickLongOp::ObjectPropertyStringLength {
                        object: instruction.op1,
                        cache_ip,
                        result: strlen.result,
                        next_target: QuickLongTarget::unresolved(ip)?,
                        resume_ip,
                    }
                } else {
                    add_mask_slot(&mut long_output_mask, instruction.result, total_slots)?;
                    ip += 1;
                    QuickLongOp::ObjectPropertyLong {
                        object: instruction.op1,
                        cache_ip,
                        result: instruction.result,
                        next_target: QuickLongTarget::unresolved(ip)?,
                        resume_ip,
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
                    let mut object_long_arguments = [
                        QuickObjectLongArgument::Long(QuickLongOperand::Const(0));
                        8
                    ];
                    let mut has_string_argument = false;
                    let mut cursor = ip + 1;
                    for argument_index in 0..argument_count {
                        let send = *op_array.instructions.get(cursor)?;
                        if !matches!(send.opcode, OpCode::SendVal | OpCode::SendVarEx)
                            || send.op2 as usize != argument_index + 1
                        {
                            return None;
                        }
                        if send.op1_type == OpType::Cv
                            && string_key_assignment_mask & (1u64 << send.op1) != 0
                        {
                            add_mask_slot(&mut string_input_mask, send.op1, total_slots)?;
                            object_long_arguments[argument_index] =
                                QuickObjectLongArgument::StringSlot(send.op1);
                            has_string_argument = true;
                            cursor += 1;
                            continue;
                        }
                        let argument = match send.op1_type {
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
                        arguments[argument_index] = argument;
                        object_long_arguments[argument_index] =
                            QuickObjectLongArgument::Long(argument);
                        cursor += 1;
                    }
                    let do_fcall = *op_array.instructions.get(cursor)?;
                    if do_fcall.opcode != OpCode::DoFcall {
                        return None;
                    }
                    let destination = matches!(
                        do_fcall.result_type,
                        OpType::Tmp | OpType::Var
                    )
                    .then(|| {
                        op_array
                            .instructions
                            .get(cursor + 1)
                            .copied()
                            .and_then(long_assign)
                            .and_then(|(destination, source)| {
                                (source == do_fcall.result).then_some(destination)
                            })
                    })
                    .flatten();
                    let resume_ip = ip;
                    ip = cursor + 1;
                    if destination.is_some() {
                        has_assign = true;
                        ip += 1;
                    }
                    // A consumed DoFcall TMP is dead after the skipped
                    // canonical Assign. Write the quick result directly into
                    // that CV so the hot opcode stays compact.
                    let call_result = destination.unwrap_or(do_fcall.result);
                    let call = QuickTypedMethodCall {
                        guard: outer_guard,
                        arguments,
                        argument_count: argument_count as u8,
                        next_target: QuickLongTarget::unresolved(ip)?,
                        resume_ip,
                    };
                    if has_string_argument
                        && argument_count != 0
                        && matches!(do_fcall.result_type, OpType::Tmp | OpType::Var)
                    {
                        add_mask_slot(
                            &mut long_output_mask,
                            call_result,
                            total_slots,
                        )?;
                        QuickLongOp::ObjectLongMethodCall {
                            call: QuickObjectLongMethodCall {
                                guard: outer_guard,
                                arguments: object_long_arguments,
                                argument_count: argument_count as u8,
                                next_target: call.next_target,
                                resume_ip,
                            },
                            result: call_result,
                        }
                    } else if do_fcall.result_type == OpType::Unused {
                        QuickLongOp::PropertyMethodCall { call }
                    } else if argument_count == 0
                        && matches!(do_fcall.result_type, OpType::Tmp | OpType::Var)
                    {
                        add_mask_slot(
                            &mut long_output_mask,
                            call_result,
                            total_slots,
                        )?;
                        QuickLongOp::PropertyGetterCall {
                            call,
                            result: call_result,
                        }
                    } else if argument_count != 0
                        && matches!(do_fcall.result_type, OpType::Tmp | OpType::Var)
                    {
                        add_mask_slot(
                            &mut long_output_mask,
                            call_result,
                            total_slots,
                        )?;
                        QuickLongOp::ScalarMethodCall {
                            call,
                            result: call_result,
                        }
                    } else {
                        return None;
                    }
                }
            }
            OpCode::AssignConcat => {
                if instruction.op1_type != OpType::Cv
                    || instruction.result_type != OpType::Unused
                {
                    return None;
                }
                let source = match instruction.op2_type {
                    OpType::Const => {
                        op_array
                            .literals
                            .get(instruction.op2 as usize)?
                            .as_str()?;
                        QuickStringAppendSource::Literal(instruction.op2)
                    }
                    OpType::Cv
                        if instruction.op2 != instruction.op1
                            && cv_unmodified_in_region(region, instruction.op2) =>
                    {
                        add_mask_slot(
                            &mut string_input_mask,
                            instruction.op2,
                            total_slots,
                        )?;
                        QuickStringAppendSource::Slot(instruction.op2)
                    }
                    _ => return None,
                };
                add_mask_slot(
                    &mut string_append_mask,
                    instruction.op1,
                    total_slots,
                )?;
                has_string_append = true;
                let resume_ip = ip;
                ip += 1;
                QuickLongOp::StringAppend {
                    destination: instruction.op1,
                    source,
                    next_target: QuickLongTarget::unresolved(ip)?,
                    resume_ip,
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
                    add_mask_slot(&mut long_output_mask, instruction.op1, total_slots)?;
                    has_assign = true;
                    ip += 1;
                    if instruction.op2_type == OpType::Const {
                        QuickLongOp::AssignLongLiteral {
                            destination: instruction.op1,
                            value: long_literal(op_array, instruction.op2)?,
                            next_target: QuickLongTarget::unresolved(ip)?,
                        }
                    } else {
                        let source = long_slot(instruction.op2_type, instruction.op2)?;
                        add_mask_slot(&mut long_input_mask, source, total_slots)?;
                        QuickLongOp::Assign {
                            destination: instruction.op1,
                            source,
                            next_target: QuickLongTarget::unresolved(ip)?,
                        }
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
                if closed_loop && ip + 1 == backedge_ip {
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
                if closed_loop && ip == backedge_ip {
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
            | QuickLongOp::BranchUnlessLe { resume_ip, .. }
            | QuickLongOp::TraceGuard { resume_ip, .. }
            | QuickLongOp::ModConst { resume_ip, .. }
            | QuickLongOp::JsonProjectionStep { resume_ip, .. }
            | QuickLongOp::FetchArrayLong { resume_ip, .. }
            | QuickLongOp::ObjectPropertyLong { resume_ip, .. }
            | QuickLongOp::ObjectPropertyStringLength { resume_ip, .. }
            | QuickLongOp::StoreArrayLong { resume_ip, .. }
            | QuickLongOp::ArrayPushLong { resume_ip, .. }
            | QuickLongOp::StringAppend { resume_ip, .. }
            | QuickLongOp::Add { resume_ip, .. }
            | QuickLongOp::Binary { resume_ip, .. }
            | QuickLongOp::BinaryAssign { resume_ip, .. }
            | QuickLongOp::ComposedPropertyCall { resume_ip, .. }
            | QuickLongOp::VirtualObjectArrayPipeline { resume_ip, .. }
            | QuickLongOp::PostInc { resume_ip, .. }
            | QuickLongOp::PostIncJump { resume_ip, .. }
            | QuickLongOp::PostIncLoopLt { resume_ip, .. } => resume_ip,
            QuickLongOp::PropertyMethodCall { call }
            | QuickLongOp::PropertyGetterCall { call, .. }
            | QuickLongOp::ScalarMethodCall { call, .. } => call.resume_ip,
            QuickLongOp::ObjectLongMethodCall { call, .. } => call.resume_ip,
            QuickLongOp::AddAssign { add_resume_ip, .. } => add_resume_ip,
            QuickLongOp::ConditionalAddAssign {
                condition_resume_ip,
                ..
            } => condition_resume_ip,
            QuickLongOp::AddAddAssign {
                first_resume_ip, ..
            } => first_resume_ip,
            QuickLongOp::Assign { .. } => ip - 1,
            QuickLongOp::AssignLongLiteral { .. } => ip - 1,
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

    if closed_loop && let (
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
            QuickLongOp::BranchUnlessLt { .. }
                | QuickLongOp::BranchUnlessEq { .. }
                | QuickLongOp::BranchUnlessLe { .. }
                | QuickLongOp::TraceGuard { .. }
        )
    });
    if closed_loop {
        if !(has_add
            || has_assign
            || has_internal_branch
            || has_object_call
            || has_array_push
            || has_string_append)
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
    } else {
        if ops.len() < 2 || !(has_add || has_assign) {
            return None;
        }
        long_input_mask = straight_long_region_inputs(&ops)?;
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

    // Compiler temporaries are single-definition values inside this OpArray.
    // If this region produces a TMP, every valid use is dominated by that
    // definition and the stale frame representation is not an entry input.
    // Keep CV read/write overlap intact because CVs carry loop state.
    let cv_mask = if op_array.num_cvs == 64 {
        u64::MAX
    } else {
        (1u64 << op_array.num_cvs) - 1
    };
    long_input_mask &= !(long_output_mask & !cv_mask);

    if let Some(source) = typed_invariant_source.as_mut() {
        if json_fetch_mask == 0
            || json_fetch_mask
                & !(json_parent_mask | long_input_mask | json_string_source_mask)
                != 0
        {
            return None;
        }
        let mut outputs = json_fetch_mask & long_input_mask;
        while outputs != 0 {
            let result = outputs.trailing_zeros() as u16;
            outputs &= outputs - 1;
            let path = json_paths
                .get(result as usize)?
                .as_ref()?
                .clone();
            if path.is_empty() {
                return None;
            }
            source.long_output_mask |= 1u64 << result;
            source.projections.push(QuickTypedInvariantProjection {
                path: path.into_boxed_slice(),
                result,
                kind: QuickInvariantValueKind::Long,
            });
        }
        let mut string_sources = json_string_source_mask;
        while string_sources != 0 {
            let result = string_sources.trailing_zeros() as u16;
            string_sources &= string_sources - 1;
            let path = json_paths.get(result as usize)?.as_ref()?.clone();
            source.string_output_mask |= 1u64 << result;
            source.projections.push(QuickTypedInvariantProjection {
                path: path.into_boxed_slice(),
                result,
                kind: QuickInvariantValueKind::String,
            });
        }
        for (result, path) in json_string_length_paths.iter().enumerate() {
            let Some(path) = path else {
                continue;
            };
            source.long_output_mask |= 1u64 << result;
            source.projections.push(QuickTypedInvariantProjection {
                path: path.clone().into_boxed_slice(),
                result: result as u16,
                kind: QuickInvariantValueKind::StringLength,
            });
        }
        if source.projections.is_empty() {
            return None;
        }
    }

    let long_mask = long_input_mask | long_output_mask;
    if long_mask & bool_output_mask != 0
        || array_input_mask & (long_mask | bool_output_mask) != 0
        || array_output_mask & (long_mask | bool_output_mask) != 0
        || string_input_mask
            & (long_mask
                | bool_output_mask
                | array_input_mask
                | array_output_mask
                | string_append_mask)
            != 0
        || string_output_mask & !string_input_mask != 0
        || string_append_mask
            & (long_mask
                | bool_output_mask
                | array_input_mask
                | array_output_mask
                | string_output_mask)
            != 0
        || object_input_mask
            & (long_mask
                | bool_output_mask
                | array_input_mask
                | array_output_mask
                | string_input_mask
                | string_append_mask)
            != 0
    {
        return None;
    }
    let involved_mask = long_input_mask
        | long_output_mask
        | bool_output_mask
        | array_input_mask
        | array_output_mask
        | string_input_mask
        | string_output_mask
        | string_append_mask
        | object_input_mask;

    let array_update_fusions = detect_array_update_fusions(&ops);
    let mut plan = QuickLongOpsLoop {
        header_ip,
        backedge_ip,
        ops,
        array_update_fusions,
        entry_op,
        op_ips,
        long_input_mask,
        long_output_mask,
        bool_output_mask,
        array_input_mask,
        array_output_mask,
        string_input_mask,
        string_output_mask,
        string_append_mask,
        object_input_mask,
        typed_invariant_source,
        string_cache_capacity: string_cache_capacity as u8,
        involved_mask,
        straight_array_kernel: None,
        #[cfg(all(
            feature = "jit-prototype",
            any(
                all(target_arch = "aarch64", target_os = "macos"),
                all(target_arch = "x86_64", target_os = "linux")
            )
        ))]
        native_jit: crate::jit::QuickLongOpsJitCache::new(),
    };
    plan.straight_array_kernel = detect_straight_array_region_kernel(&plan);
    Some(plan)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::compiler::compile::Compiler;
    use crate::compiler::make_user_function;
    use crate::lexer::Lexer;
    use crate::parser::Parser;
    use crate::vm::function::{
        ComposedScalarDoubleFunctionPlan, ComposedScalarDoubleOp, ScalarDoubleCall,
        ScalarDoubleFunctionPlan, ScalarDoubleOp, ScalarDoubleOpKind, ScalarDoubleProgram,
        ScalarDoubleSelect, ScalarDoubleSource, ScalarLongConditionKind,
    };
    use crate::vm::planner::BlockPlan;

    fn dynamic_double_argument(register: u8) -> QuickDoubleArgumentProgram {
        QuickDoubleArgumentProgram {
            operations: vec![QuickDoubleArgumentOp {
                kind: ScalarDoubleOpKind::Add,
                lhs: QuickDoubleSource::Induction,
                rhs: QuickDoubleSource::Constant(0.5),
            }]
            .into_boxed_slice(),
            outputs: [
                QuickDoubleSource::Temporary(register),
                QuickDoubleSource::Constant(0.0),
                QuickDoubleSource::Constant(0.0),
                QuickDoubleSource::Constant(0.0),
                QuickDoubleSource::Constant(0.0),
                QuickDoubleSource::Constant(0.0),
                QuickDoubleSource::Constant(0.0),
                QuickDoubleSource::Constant(0.0),
            ],
            output_count: 1,
            input_slots: [u16::MAX; 8],
            input_count: 0,
        }
    }

    #[test]
    fn forwards_dynamic_double_argument_used_before_register_overwrite() {
        let arguments = dynamic_double_argument(0);
        let leaf = ScalarDoubleFunctionPlan::new(
            1,
            ScalarDoubleProgram {
                operations: vec![ScalarDoubleOp {
                    kind: ScalarDoubleOpKind::Add,
                    lhs: ScalarDoubleSource::Input(0),
                    rhs: ScalarDoubleSource::Constant(1.0),
                }]
                .into_boxed_slice(),
                output: ScalarDoubleSource::Temporary(0),
            },
        );

        assert_eq!(arguments.register_forwardable_output_mask(&leaf), 1);
    }

    #[test]
    fn retains_buffer_when_x86_rhs_would_be_overwritten() {
        let arguments = dynamic_double_argument(0);
        let leaf = ScalarDoubleFunctionPlan::new(
            1,
            ScalarDoubleProgram {
                operations: vec![ScalarDoubleOp {
                    kind: ScalarDoubleOpKind::Subtract,
                    lhs: ScalarDoubleSource::Constant(10.0),
                    rhs: ScalarDoubleSource::Input(0),
                }]
                .into_boxed_slice(),
                output: ScalarDoubleSource::Temporary(0),
            },
        );

        assert_eq!(arguments.register_forwardable_output_mask(&leaf), 0);
    }

    #[test]
    fn flattens_guarded_double_leaf_with_target_neutral_source_remapping() {
        let composed = ComposedScalarDoubleFunctionPlan {
            public_args: 1,
            operations: vec![
                ComposedScalarDoubleOp::Call(ScalarDoubleCall {
                    guard: ScalarLongCallGuard::FunctionCache { cache_ip: 0 },
                    arguments: vec![ScalarDoubleSource::Input(0)].into_boxed_slice(),
                }),
                ComposedScalarDoubleOp::Arithmetic(ScalarDoubleOp {
                    kind: ScalarDoubleOpKind::Add,
                    lhs: ScalarDoubleSource::Temporary(0),
                    rhs: ScalarDoubleSource::Constant(3.0),
                }),
            ]
            .into_boxed_slice(),
            output: ScalarDoubleSource::Temporary(1),
        };
        let leaf = ScalarDoubleFunctionPlan::new(
            1,
            ScalarDoubleProgram {
                operations: vec![ScalarDoubleOp {
                    kind: ScalarDoubleOpKind::Multiply,
                    lhs: ScalarDoubleSource::Input(0),
                    rhs: ScalarDoubleSource::Constant(2.0),
                }]
                .into_boxed_slice(),
                output: ScalarDoubleSource::Temporary(0),
            },
        );

        let flattened = compose_scalar_double_program(
            &composed,
            &[
                Some(ResolvedScalarDoubleProgram {
                    public_args: leaf.public_args,
                    program: &leaf.program,
                    select: leaf.select,
                }),
                None,
            ],
        )
        .unwrap();
        assert_eq!(flattened.program.operations.len(), 2);
        assert!(matches!(
            flattened.program.operations[0],
            ScalarDoubleOp {
                kind: ScalarDoubleOpKind::Multiply,
                lhs: ScalarDoubleSource::Input(0),
                rhs: ScalarDoubleSource::Constant(2.0),
            }
        ));
        assert!(matches!(
            flattened.program.operations[1],
            ScalarDoubleOp {
                kind: ScalarDoubleOpKind::Add,
                lhs: ScalarDoubleSource::Temporary(0),
                rhs: ScalarDoubleSource::Constant(3.0),
            }
        ));
        assert_eq!(flattened.program.output, ScalarDoubleSource::Temporary(1));
        assert!(flattened.select.is_none());
    }

    fn conditional_double_leaf() -> ScalarDoubleFunctionPlan {
        ScalarDoubleFunctionPlan::new_conditional(
            2,
            ScalarDoubleProgram {
                operations: vec![
                    ScalarDoubleOp {
                        kind: ScalarDoubleOpKind::Multiply,
                        lhs: ScalarDoubleSource::Input(0),
                        rhs: ScalarDoubleSource::Constant(1.5),
                    },
                    ScalarDoubleOp {
                        kind: ScalarDoubleOpKind::Subtract,
                        lhs: ScalarDoubleSource::Input(0),
                        rhs: ScalarDoubleSource::Constant(1.0),
                    },
                ]
                .into_boxed_slice(),
                output: ScalarDoubleSource::Temporary(0),
            },
            ScalarDoubleSelect {
                kind: ScalarLongConditionKind::LessThan,
                lhs: ScalarDoubleSource::Input(0),
                rhs: ScalarDoubleSource::Input(1),
                shared_operation_count: 0,
                when_true_operation_count: 1,
                when_false_operation_count: 1,
                when_true: ScalarDoubleSource::Temporary(0),
                when_false: ScalarDoubleSource::Temporary(1),
                merge_result: false,
            },
        )
    }

    #[test]
    fn flattens_one_conditional_double_leaf_into_a_common_suffix() {
        let composed = ComposedScalarDoubleFunctionPlan {
            public_args: 2,
            operations: vec![
                ComposedScalarDoubleOp::Call(ScalarDoubleCall {
                    guard: ScalarLongCallGuard::FunctionCache { cache_ip: 0 },
                    arguments: vec![ScalarDoubleSource::Input(0), ScalarDoubleSource::Input(1)]
                        .into_boxed_slice(),
                }),
                ComposedScalarDoubleOp::Arithmetic(ScalarDoubleOp {
                    kind: ScalarDoubleOpKind::Add,
                    lhs: ScalarDoubleSource::Temporary(0),
                    rhs: ScalarDoubleSource::Constant(3.0),
                }),
            ]
            .into_boxed_slice(),
            output: ScalarDoubleSource::Temporary(1),
        };
        let leaf = conditional_double_leaf();

        let flattened = compose_scalar_double_program(
            &composed,
            &[
                Some(ResolvedScalarDoubleProgram {
                    public_args: leaf.public_args,
                    program: &leaf.program,
                    select: leaf.select,
                }),
                None,
            ],
        )
        .expect("one conditional callee should flatten");

        let select = flattened.select.expect("flattened merge select");
        assert!(select.merge_result);
        assert_eq!(select.operation_ranges(3), Some((0, 1, 2)));
        assert_eq!(select.when_true, ScalarDoubleSource::Temporary(0));
        assert_eq!(select.when_false, ScalarDoubleSource::Temporary(1));
        assert!(matches!(
            flattened.program.operations[2],
            ScalarDoubleOp {
                kind: ScalarDoubleOpKind::Add,
                lhs: ScalarDoubleSource::Selection,
                rhs: ScalarDoubleSource::Constant(3.0),
            }
        ));
        assert_eq!(flattened.program.output, ScalarDoubleSource::Temporary(2));
    }

    #[test]
    fn rejects_two_conditional_double_callees_from_one_flattened_region() {
        let composed = ComposedScalarDoubleFunctionPlan {
            public_args: 2,
            operations: vec![
                ComposedScalarDoubleOp::Call(ScalarDoubleCall {
                    guard: ScalarLongCallGuard::FunctionCache { cache_ip: 0 },
                    arguments: vec![ScalarDoubleSource::Input(0), ScalarDoubleSource::Input(1)]
                        .into_boxed_slice(),
                }),
                ComposedScalarDoubleOp::Call(ScalarDoubleCall {
                    guard: ScalarLongCallGuard::FunctionCache { cache_ip: 1 },
                    arguments: vec![ScalarDoubleSource::Input(0), ScalarDoubleSource::Input(1)]
                        .into_boxed_slice(),
                }),
                ComposedScalarDoubleOp::Arithmetic(ScalarDoubleOp {
                    kind: ScalarDoubleOpKind::Add,
                    lhs: ScalarDoubleSource::Temporary(0),
                    rhs: ScalarDoubleSource::Temporary(1),
                }),
            ]
            .into_boxed_slice(),
            output: ScalarDoubleSource::Temporary(2),
        };
        let leaf = conditional_double_leaf();
        let resolved = ResolvedScalarDoubleProgram {
            public_args: leaf.public_args,
            program: &leaf.program,
            select: leaf.select,
        };

        assert!(
            compose_scalar_double_program(&composed, &[Some(resolved), Some(resolved), None],)
                .is_none()
        );
    }

    #[test]
    fn rejects_flattened_double_body_beyond_shared_register_capacity() {
        let composed = ComposedScalarDoubleFunctionPlan {
            public_args: 1,
            operations: vec![
                ComposedScalarDoubleOp::Call(ScalarDoubleCall {
                    guard: ScalarLongCallGuard::FunctionCache { cache_ip: 0 },
                    arguments: vec![ScalarDoubleSource::Input(0)].into_boxed_slice(),
                }),
                ComposedScalarDoubleOp::Arithmetic(ScalarDoubleOp {
                    kind: ScalarDoubleOpKind::Add,
                    lhs: ScalarDoubleSource::Temporary(0),
                    rhs: ScalarDoubleSource::Constant(1.0),
                }),
            ]
            .into_boxed_slice(),
            output: ScalarDoubleSource::Temporary(1),
        };
        let mut leaf_operations = Vec::new();
        for index in 0..8 {
            leaf_operations.push(ScalarDoubleOp {
                kind: ScalarDoubleOpKind::Add,
                lhs: if index == 0 {
                    ScalarDoubleSource::Input(0)
                } else {
                    ScalarDoubleSource::Temporary(index - 1)
                },
                rhs: ScalarDoubleSource::Constant(1.0),
            });
        }
        let leaf = ScalarDoubleFunctionPlan::new(
            1,
            ScalarDoubleProgram {
                operations: leaf_operations.into_boxed_slice(),
                output: ScalarDoubleSource::Temporary(7),
            },
        );

        assert!(
            compose_scalar_double_program(
                &composed,
                &[
                    Some(ResolvedScalarDoubleProgram {
                        public_args: leaf.public_args,
                        program: &leaf.program,
                        select: leaf.select,
                    }),
                    None,
                ],
            )
            .is_none()
        );
    }

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

    #[test]
    fn detects_exact_double_scalar_call_accumulation() {
        let main = compile_main(
            "<?php
function calculateFloat(float $a, float $b, float $c): float {
    return ($a + $b) * $c;
}
$scale = 2.0;
$total = 0.0;
for ($i = 0; $i < 100; $i++) {
    $total += calculateFloat(1.5, 2.5, $scale);
}
",
        );
        let plan = main
            .op_array
            .block_plans
            .iter()
            .find_map(|plan| match plan {
                BlockPlan::QuickDoubleCallAccumulate(plan) => Some(plan),
                _ => None,
            })
            .expect("compiler should select the Double call-accumulate loop");
        assert_eq!(plan.argument_program.output_count, 3);
        assert_eq!(plan.argument_program.input_count, 1);
        assert_eq!(plan.argument_program.input_slots[0], 0);
        assert!(matches!(
            plan.argument_program.outputs[0],
            QuickDoubleSource::Constant(1.5)
        ));
        assert!(matches!(
            plan.argument_program.outputs[1],
            QuickDoubleSource::Constant(2.5)
        ));
        assert!(matches!(
            plan.argument_program.outputs[2],
            QuickDoubleSource::Input(0)
        ));
        assert_eq!(plan.accumulator_cv, 1);
        assert_eq!(plan.induction_cv, 2);
    }

    #[test]
    fn detects_monomorphic_double_method_accumulation() {
        let main = compile_main(
            "<?php
class FloatCalculator {
    public function calculate(float $a, float $b, float $c): float {
        return (($a + $b) * $c) - 2.0;
    }
}
$calculator = new FloatCalculator();
$total = 0.0;
for ($i = 0; $i < 100; $i++) {
    $total += $calculator->calculate(1.5, 2.5, 2.0);
}
",
        );
        let plan = main
            .op_array
            .block_plans
            .iter()
            .find_map(|plan| match plan {
                BlockPlan::QuickDoubleCallAccumulate(plan) => Some(plan),
                _ => None,
            })
            .expect("compiler should select the Double method/accumulate loop");
        assert!(matches!(
            plan.guard,
            ScalarLongCallGuard::MethodCache {
                receiver_slot: 0,
                ..
            }
        ));
        assert_eq!(plan.argument_program.output_count, 3);
        assert_eq!(plan.argument_program.input_count, 0);
        assert_eq!(plan.argument_program.outputs[0], QuickDoubleSource::Constant(1.5));
        assert_eq!(plan.argument_program.outputs[1], QuickDoubleSource::Constant(2.5));
        assert_eq!(plan.argument_program.outputs[2], QuickDoubleSource::Constant(2.0));
        assert_eq!(plan.accumulator_cv, 1);
        assert_eq!(plan.induction_cv, 2);
    }

    #[test]
    fn detects_induction_and_invariant_double_argument_expressions() {
        let main = compile_main(
            "<?php
function calculateFloat(float $a, float $b, float $c): float {
    return (($a + $b) * $c) - 2.0;
}
$scale = 2.0;
$total = 0.0;
for ($i = 0; $i < 100; $i++) {
    $total += calculateFloat($i * 0.5, $scale + 1.0, 2.0);
}
",
        );
        let plan = main
            .op_array
            .block_plans
            .iter()
            .find_map(|plan| match plan {
                BlockPlan::QuickDoubleCallAccumulate(plan) => Some(plan),
                _ => None,
            })
            .expect("compiler should retain scalar argument expressions");
        let arguments = &plan.argument_program;
        assert_eq!(arguments.operations.len(), 2);
        assert_eq!(arguments.input_count, 1);
        assert_eq!(arguments.input_slots[0], 0);
        assert_eq!(arguments.operations[0].kind, ScalarDoubleOpKind::Multiply);
        assert_eq!(arguments.operations[0].lhs, QuickDoubleSource::Induction);
        assert_eq!(
            arguments.operations[0].rhs,
            QuickDoubleSource::Constant(0.5)
        );
        assert_eq!(arguments.operations[1].kind, ScalarDoubleOpKind::Add);
        assert_eq!(arguments.operations[1].lhs, QuickDoubleSource::Input(0));
        assert_eq!(
            arguments.operations[1].rhs,
            QuickDoubleSource::Constant(1.0)
        );
        assert_eq!(arguments.outputs[0], QuickDoubleSource::Temporary(0));
        assert_eq!(arguments.outputs[1], QuickDoubleSource::Temporary(1));
        assert_eq!(arguments.outputs[2], QuickDoubleSource::Constant(2.0));
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
    fn detects_invariant_json_decode_long_projections() {
        let plan = long_ops_plan(
            "<?php
$json = '{\"age\":30,\"scores\":[95,87]}';
$sum = 0;
for ($i = 0; $i < 100; $i++) {
    $row = json_decode($json, true);
    $sum = $sum + $row['age'] + $row['scores'][0] + $row['scores'][1];
}
",
        );
        let source = plan
            .typed_invariant_source
            .as_ref()
            .expect("stable associative json_decode should become a prelude");
        assert_eq!(source.projections.len(), 3);
        assert_eq!(source.long_output_mask.count_ones(), 3);
        assert!(plan.ops.iter().any(|operation| matches!(
            operation,
            QuickLongOp::JsonProjectionStep { .. }
        )));
        assert!(source.projections.iter().any(|output| {
            matches!(
                output.path.as_ref(),
                [QuickInvariantPathElement::StringLiteral(_)]
            )
        }));
        assert!(source.projections.iter().any(|output| {
            matches!(
                output.path.as_ref(),
                [
                    QuickInvariantPathElement::StringLiteral(_),
                    QuickInvariantPathElement::Integer(0)
                ]
            )
        }));
    }

    #[test]
    fn derives_invariant_string_length_as_a_long_projection() {
        let plan = long_ops_plan(
            "<?php
$json = '{\"name\":\"hyper-optimized\"}';
$sum = 0;
for ($i = 0; $i < 100; $i++) {
    $row = json_decode($json, true);
    $sum = $sum + strlen($row['name']);
}
",
        );
        let source = plan
            .typed_invariant_source
            .as_ref()
            .expect("fixed string projection should become a typed prelude");
        assert_eq!(source.string_output_mask.count_ones(), 1);
        assert_eq!(source.long_output_mask.count_ones(), 1);
        assert!(source
            .projections
            .iter()
            .any(|projection| projection.kind == QuickInvariantValueKind::String));
        assert!(source
            .projections
            .iter()
            .any(|projection| projection.kind == QuickInvariantValueKind::StringLength));
    }

    #[test]
    fn feeds_invariant_json_double_projection_into_scalar_call_ir() {
        let main = compile_main(
            "<?php
function scaleJson(float $value): float {
    return $value * 1.5;
}
$json = '{\"value\":1.25}';
$total = 0.0;
for ($i = 0; $i < 100; $i++) {
    $row = json_decode($json, true);
    $total += scaleJson($row['value']);
}
",
        );
        let plan = main
            .op_array
            .block_plans
            .iter()
            .find_map(|plan| match plan {
                BlockPlan::QuickDoubleCallAccumulate(plan) => Some(plan),
                _ => None,
            })
            .unwrap_or_else(|| {
                panic!(
                    "compiler should select a typed Double source; instructions: {:#?}",
                    main.op_array.instructions
                )
            });
        let source = plan
            .typed_invariant_source
            .as_ref()
            .expect("associative JSON source should be retained");
        assert_eq!(source.double_output_mask.count_ones(), 1);
        assert_eq!(source.projections.len(), 1);
        assert_eq!(
            source.projections[0].kind,
            QuickInvariantValueKind::Double
        );
        assert_eq!(plan.argument_program.input_count, 1);
        assert_eq!(
            plan.argument_program.input_slots[0],
            source.projections[0].result
        );
    }

    #[test]
    #[cfg(feature = "quick-loops")]
    fn selects_straight_array_application_region_from_general_typed_ops() {
        let main = compile_main(
            "<?php
$row = ['a' => 2, 'b' => 3, 'c' => 4];
$a = 10;
$b = 20;
$c = 30;
$a = $a + $row['a'];
$b = $b + $row['b'];
$c = $c + $row['c'];
echo $a + $b + $c;
",
        );
        let (entry_ip, entry) = main
            .op_array
            .instructions
            .iter()
            .enumerate()
            .find(|(_, instruction)| {
                instruction.opcode == OpCode::FetchDimR
                    && instruction.extended_value != 0
            })
            .expect("compiler should mark a straight typed region entry");
        let block_idx = entry.extended_value as usize - 1;
        let BlockPlan::QuickLongOps(plan) = &main.op_array.block_plans[block_idx]
        else {
            panic!("marked entry must reference a typed region plan");
        };
        assert_eq!(plan.header_ip, entry_ip);
        assert!(plan.straight_array_kernel.is_some());

        let first_fetch_result = entry.result;
        assert_eq!(
            plan.long_input_mask & (1u64 << first_fetch_result),
            0,
            "a temporary produced inside the region is not an entry input"
        );
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
                call: QuickTypedMethodCall {
                    argument_count: 0,
                    ..
                },
                ..
            }
        )));
        assert!(plan.ops.iter().any(|operation| matches!(
            operation,
            QuickLongOp::ComposedPropertyCall { .. }
        )));
    }

    #[test]
    fn detects_guarded_invariant_object_property_reads_in_long_ops_loop() {
        let plan = long_ops_plan(
            "<?php
$row = json_decode('{\"value\":11,\"name\":\"alpha\"}');
$sum = 0;
for ($i = 0; $i < 100; $i++) {
    $sum += $row->value + strlen($row->name);
}
",
        );
        assert_ne!(plan.object_input_mask, 0);
        assert!(plan.ops.iter().any(|operation| matches!(
            operation,
            QuickLongOp::ObjectPropertyLong { .. }
        )));
        assert!(plan.ops.iter().any(|operation| matches!(
            operation,
            QuickLongOp::ObjectPropertyStringLength { .. }
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
            } if long_input_mask == 0
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
        let body = ScalarLongFunctionPlan::new(
            2,
            ScalarLongProgram {
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
            None,
        );

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
            QuickLongTerm::ScalarCallTree {
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
    fn detects_nested_scalar_function_accumulation_as_call_tree() {
        let plan = quick_plan(
            "<?php
function addNative($left, $right) { return $left + $right; }
function mulNative($left, $right) { return $left * $right; }
$sum = 0;
for ($i = 0; $i < 100; $i++) {
    $sum += addNative($i + 1, mulNative($i, 2));
}
",
        );
        assert!(matches!(
            plan.term,
            QuickLongTerm::ScalarCallTree {
                argument_count: 2,
                long_input_mask,
                object_input_mask: 0,
                guard,
                do_fcall_ip,
                ..
            } if long_input_mask == 1u64 << 1
                && matches!(guard, ScalarLongCallGuard::FunctionCache { .. })
                && do_fcall_ip > guard.cache_ip()
        ));
    }

    #[test]
    fn detects_cold_strict_branch_as_tail_trace_guard() {
        let plan = quick_plan(
            "<?php
function routeStandalone(int $value): int { return ($value * 2) + 1; }
$sum = 0;
for ($i = 0; $i < 100; $i++) {
    $sum += routeStandalone($i);
    if ($i === -1) {
        echo 'never';
    }
}
",
        );
        assert!(matches!(
            plan.tail_guard,
            Some(QuickLongTraceGuard {
                kind: ScalarLongConditionKind::Equal,
                lhs: QuickLongOperand::Slot(lhs),
                rhs: QuickLongOperand::Const(-1),
                expected: false,
                condition_tmp: Some(_),
                resume_ip,
            }) if lhs == plan.induction_cv && resume_ip < plan.increment_ip
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
    fn detects_long_array_push_as_typed_op() {
        let plan = long_ops_plan(
            "<?php
$values = [];
for ($i = 0; $i < 100; $i++) {
    $values[] = $i;
}
",
        );
        assert!(matches!(
            plan.ops.as_slice(),
            [
                QuickLongOp::BranchUnlessLt { .. },
                QuickLongOp::ArrayPushLong {
                    value: QuickLongOperand::Slot(_),
                    ..
                },
                QuickLongOp::PostIncLoopLt { .. },
            ]
        ));
        assert_ne!(plan.array_output_mask, 0);
        assert_eq!(
            plan.array_output_mask
                & (plan.long_input_mask
                    | plan.long_output_mask
                    | plan.bool_output_mask
                    | plan.array_input_mask),
            0
        );
    }

    #[test]
    fn detects_literal_and_invariant_string_append_as_typed_ops() {
        for (setup, expression, expected_slot) in [
            ("", "'x'", None),
            ("$suffix = 'yz';", "$suffix", Some(0)),
        ] {
            let plan = long_ops_plan(&format!(
                "<?php
{setup}
$value = '';
for ($i = 0; $i < 100; $i++) {{
    $value .= {expression};
}}
"
            ));
            assert!(matches!(
                plan.ops.as_slice(),
                [
                    QuickLongOp::BranchUnlessLt { .. },
                    QuickLongOp::StringAppend { source, .. },
                    QuickLongOp::PostIncLoopLt { .. },
                ] if match expected_slot {
                    Some(slot) => *source == QuickStringAppendSource::Slot(slot),
                    None => matches!(source, QuickStringAppendSource::Literal(_)),
                }
            ));
            assert_ne!(plan.string_append_mask, 0);
            assert_eq!(plan.string_append_mask & plan.string_input_mask, 0);
        }
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
    fn fuses_general_binary_results_materialized_into_loop_cvs() {
        let plan = long_ops_plan(
            "<?php
$last = 0;
$product = 0;
for ($i = 0; $i < 100; $i++) {
    $last = 20 + ($i % 400);
    $product = $i * 73;
}
echo $last + $product;
",
        );
        assert_eq!(
            plan.ops
                .iter()
                .filter(|operation| matches!(operation, QuickLongOp::BinaryAssign { .. }))
                .count(),
            2,
            "{:#?}",
            plan.ops
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
    fn detects_composed_bitwise_integer_hash_key_as_typed_ops() {
        let plan = long_ops_plan(
            "<?php
$values = [1000000 => 3, 1104515245 => 5];
$sum = 0;
for ($i = 0; $i < 2; $i++) {
    $key = (($i * 1103515245) & 2147483647) + 1000000;
    $sum += $values[$key];
}
",
        );
        assert!(plan.ops.iter().any(|operation| matches!(
            operation,
            QuickLongOp::Binary {
                kind: ScalarLongOpKind::BitwiseAnd,
                ..
            }
        )), "{:#?}", plan.ops);
        assert!(plan.ops.iter().any(|operation| matches!(
            operation,
            QuickLongOp::FetchArrayLong {
                index: QuickArrayIndex::Long(QuickLongOperand::Slot(_)),
                ..
            }
        )), "{:#?}", plan.ops);
        assert!(plan.ops.iter().any(|operation| matches!(
            operation,
            QuickLongOp::AddAssign { .. }
        )), "{:#?}", plan.ops);
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
    fn fuses_existing_dynamic_hash_entry_update_without_structural_writes() {
        let plan = long_ops_plan(
            "<?php
$values = ['left' => 3, 'right' => 5];
$key = 'left';
for ($i = 0; $i < 100; $i++) {
    $values[$key] = $values[$key] + $i;
    if (($i % 2) == 0) {
        $key = 'right';
    } else {
        $key = 'left';
    }
}
",
        );
        let fetch = plan
            .ops
            .iter()
            .position(|operation| matches!(operation, QuickLongOp::FetchArrayLong { .. }))
            .expect("dynamic hash fetch");
        let fusion = plan.array_update_fusions[fetch].expect("array update fusion");
        assert_eq!(fusion.kind, ScalarLongOpKind::Add);
        assert!(fusion.next_target.op_index().is_some());
    }

    #[test]
    fn detects_mixed_string_method_and_control_flow_method_in_one_hash_loop() {
        let plan = long_ops_plan(
            "<?php
class Mixer {
    public function score(int $value, string $key): int {
        return $value + strlen($key);
    }
    public function accepted(int $value, int $sequence): int {
        if (($value % 11) == 0 || ($sequence % 17) == 0) { return 1; }
        return 0;
    }
}
$mixer = new Mixer();
$values = ['left' => 0, 'right' => 0];
$key = 'left';
$accepted = 0;
$needle = -1;
for ($i = 0; $i < 100; $i++) {
    if (($i % 2) == 0) { $key = 'right'; } else { $key = 'left'; }
    $score = $mixer->score($i, $key);
    $values[$key] = $values[$key] + $score;
    $isAccepted = $mixer->accepted($score, $i);
    $accepted = $accepted + $isAccepted;
    if ($i === $needle) { echo 'never'; }
}
",
        );
        assert!(plan
            .ops
            .iter()
            .any(|operation| matches!(
                operation,
                QuickLongOp::ObjectLongMethodCall { .. }
            )));
        assert!(plan
            .ops
            .iter()
            .any(|operation| matches!(
                operation,
                QuickLongOp::ScalarMethodCall { .. }
            )));
        assert!(!plan
            .ops
            .iter()
            .any(|operation| matches!(operation, QuickLongOp::Assign { .. })));
        assert!(plan.ops.iter().any(|operation| matches!(
            operation,
            QuickLongOp::TraceGuard {
                kind: ScalarLongConditionKind::Equal,
                expected: false,
                ..
            }
        )));
        assert!(plan.array_update_fusions.iter().any(Option::is_some));
    }

    #[test]
    fn detects_strict_cold_edge_inside_general_long_ops_loop() {
        let plan = long_ops_plan(
            "<?php
$needle = -1;
$sum = 0;
$count = 0;
for ($i = 0; $i < 100; $i++) {
    $sum = $sum + $i;
    $count = $count + 1;
    if ($i === $needle) {
        echo 'never';
    }
}
",
        );
        let guard_index = plan
            .ops
            .iter()
            .position(|operation| matches!(operation, QuickLongOp::TraceGuard { .. }))
            .expect("strict cold edge should remain inside the general typed loop");
        assert!(matches!(
            plan.ops[guard_index],
            QuickLongOp::TraceGuard {
                kind: ScalarLongConditionKind::Equal,
                expected: false,
                next_target,
                resume_ip,
                ..
            } if next_target.op_index() == Some(guard_index + 1)
                && resume_ip < plan.backedge_ip
        ));
        assert!(matches!(
            plan.ops.last(),
            Some(QuickLongOp::PostIncLoopLt { .. })
        ));
    }

    #[test]
    fn structural_array_push_disables_cached_entry_pointer_fusion() {
        let plan = long_ops_plan(
            "<?php
$values = ['left' => 3];
$key = 'left';
for ($i = 0; $i < 100; $i++) {
    $values[$key] = $values[$key] + 1;
    $values[] = $i;
}
",
        );
        assert!(plan
            .ops
            .iter()
            .any(|operation| matches!(operation, QuickLongOp::StoreArrayLong { .. })));
        assert!(plan.array_update_fusions.iter().all(Option::is_none));
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
