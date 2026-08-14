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
    QuickForeachObjectPropertyAccumulateLoop, detect_foreach_object_property_accumulate_loop,
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

#[derive(Debug, Clone, Copy)]
pub struct QuickVirtualDeclaredPropertyRead {
    pub property_literal: u16,
    pub result: u16,
}

impl QuickVirtualDeclaredPropertyRead {
    pub const EMPTY: Self = Self {
        property_literal: 0,
        result: 0,
    };
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
            QuickDoubleSource::Temporary(index) => {
                self.operations
                    .get(index as usize)
                    .is_some_and(|operation| {
                        self.source_depends_on_induction(operation.lhs)
                            || self.source_depends_on_induction(operation.rhs)
                    })
            }
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
            .filter(|output| self.source_depends_on_induction(*output) == induction_dependent)
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
            for (operation_index, operation) in leaf.program.operations.iter().copied().enumerate()
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
    if plan.operations.len() > 16 || plan.operations.len() > resolved_programs.len() {
        return None;
    }

    fn remap_composed_source(
        source: ScalarDoubleSource,
        results: &[Option<ScalarDoubleSource>],
    ) -> Option<ScalarDoubleSource> {
        match source {
            ScalarDoubleSource::Input(index) => Some(ScalarDoubleSource::Input(index)),
            ScalarDoubleSource::Constant(value) => Some(ScalarDoubleSource::Constant(value)),
            ScalarDoubleSource::Temporary(index) => results.get(index as usize).copied().flatten(),
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
                let resolved = resolved_programs.get(composed_index).copied().flatten()?;
                if resolved.public_args as usize != call.arguments.len()
                    || operations.len() + resolved.program.operations.len() > MAX_OPERATIONS
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
                    ScalarDoubleSource::Input(index) if (index as usize) < call.arguments.len() => {
                        Some(arguments[index as usize])
                    }
                    ScalarDoubleSource::Input(_) => None,
                    ScalarDoubleSource::Constant(value) => {
                        Some(ScalarDoubleSource::Constant(value))
                    }
                    ScalarDoubleSource::Temporary(index) => leaf_start
                        .checked_add(index as usize)
                        .filter(|index| *index < leaf_start + resolved.program.operations.len())
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
    /// Insert or replace a Long at a normalized integer key in a unique COW
    /// array. Unlike `StoreArrayLong`, this operation may grow or reorder the
    /// backing storage, so planning excludes any borrowed read view of the
    /// same array from the region.
    SetArrayLong {
        array: u16,
        index: QuickLongOperand,
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
    /// PHP integer shift with canonical wrapping-count semantics.
    Shift {
        left: bool,
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
    /// A compiler-proven literal zero-argument object whose dead local is used
    /// only by the immediately following declared Long-property reads.
    VirtualDeclaredObjectReads {
        class_literal: u16,
        reads: [QuickVirtualDeclaredPropertyRead; 8],
        read_count: u8,
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
            | Self::SetArrayLong { next_target, .. }
            | Self::ArrayPushLong { next_target, .. }
            | Self::StringAppend { next_target, .. }
            | Self::Add { next_target, .. }
            | Self::Binary { next_target, .. }
            | Self::Shift { next_target, .. }
            | Self::BinaryAssign { next_target, .. }
            | Self::AddAssign { next_target, .. }
            | Self::ConditionalAddAssign { next_target, .. }
            | Self::AddAddAssign { next_target, .. }
            | Self::ComposedPropertyCall { next_target, .. }
            | Self::VirtualObjectArrayPipeline { next_target, .. }
            | Self::VirtualDeclaredObjectReads { next_target, .. }
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
    pub structural_array_output_mask: u64,
    pub string_input_mask: u64,
    pub string_output_mask: u64,
    pub string_append_mask: u64,
    /// Complete finite set of literals proven for retained dynamic String
    /// assignments, including immutable preheader CV sources. Native dispatch
    /// guards every tokenized String input against this table.
    pub finite_string_literals: [u16; QUICK_STRING_FETCH_CACHE_LIMIT],
    pub finite_string_literal_count: u8,
    pub finite_string_literal_overflow: bool,
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

include!("quick_array_plan.rs");
include!("quick_double_plan.rs");
include!("quick_scalar_plan.rs");
include!("quick_accumulate_plan.rs");
include!("quick_long_region_helpers.rs");
include!("quick_long_region_plan.rs");
include!("quick_tests.rs");
