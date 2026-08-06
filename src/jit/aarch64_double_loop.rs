//! Composed exact-Double call/accumulate loop lowering for macOS ARM64.

use super::super::memory::ExecutableMemory;
use super::{Arm64Assembler, Arm64Condition, Arm64FloatRegister, Arm64Register};
use crate::vm::function::{
    ScalarDoubleFunctionPlan, ScalarDoubleOp, ScalarDoubleOpKind, ScalarDoubleSource,
    ScalarLongConditionKind,
};
use crate::vm::quick::{QuickDoubleArgumentOp, QuickDoubleArgumentProgram, QuickDoubleSource};
use std::cell::{Cell, OnceCell};
use std::fmt;
use std::io;

const MAX_INPUTS: usize = 8;
const MAX_OPERATIONS: usize = 8;
const FIRST_TEMPORARY: u8 = 16;
const DOUBLE_SELECTION_REGISTER: u8 = 24;
const SAFEPOINT_INTERVAL: u16 = 1024;
const STATUS_COMPLETED: u32 = 0;
const STATUS_INTERRUPTED: u32 = 1;
const STATUS_SIDE_EXIT: u32 = 2;

/// Target-neutral scalar state shared with the generated loop.
#[derive(Debug, Clone, Copy, PartialEq)]
#[repr(C)]
pub struct NativeDoubleCallAccumulateState {
    pub induction: i64,
    pub bound: i64,
    pub accumulator: f64,
    pub last_term: f64,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum QuickDoubleCallAccumulateJitOutcome {
    Completed,
    Interrupted,
    SideExit,
}

#[derive(Debug)]
pub enum QuickDoubleCallAccumulateJitError {
    InvalidProgram(&'static str),
    BranchOutOfRange,
    Memory(io::Error),
    InvalidNativeStatus(u32),
}

impl fmt::Display for QuickDoubleCallAccumulateJitError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::InvalidProgram(reason) => write!(formatter, "invalid Double loop: {reason}"),
            Self::BranchOutOfRange => {
                formatter.write_str("ARM64 Double loop branch is out of range")
            }
            Self::Memory(error) => write!(formatter, "cannot create executable memory: {error}"),
            Self::InvalidNativeStatus(status) => {
                write!(formatter, "Double loop returned an unknown status {status}")
            }
        }
    }
}

impl std::error::Error for QuickDoubleCallAccumulateJitError {
    fn source(&self) -> Option<&(dyn std::error::Error + 'static)> {
        match self {
            Self::Memory(error) => Some(error),
            _ => None,
        }
    }
}

impl From<io::Error> for QuickDoubleCallAccumulateJitError {
    fn from(error: io::Error) -> Self {
        Self::Memory(error)
    }
}

/// Native ABI: x0 is state, x1 compact exact-Double inputs, x2 is the interrupt
/// flag and x3 is the writable argument buffer; w0 is the outcome. A side exit
/// publishes only iterations completed before the failing division so the VM
/// can restart at `InitFcall` exactly.
pub struct CompiledQuickDoubleCallAccumulateLoop {
    memory: ExecutableMemory,
    code: Box<[u8]>,
    input_count: usize,
    forwarded_argument_mask: u8,
}

impl CompiledQuickDoubleCallAccumulateLoop {
    pub fn compile(
        argument_plan: &QuickDoubleArgumentProgram,
        plan: &ScalarDoubleFunctionPlan,
    ) -> Result<Self, QuickDoubleCallAccumulateJitError> {
        validate_argument_plan(argument_plan, plan.public_args)?;
        validate(plan)?;
        let forwarded_argument_mask = argument_plan.register_forwardable_output_mask(plan);

        let mut assembler = Arm64Assembler::new();
        let state = Arm64Register::from_code(9);
        let inputs = Arm64Register::from_code(10);
        let interrupt = Arm64Register::from_code(11);
        let working_arguments = Arm64Register::from_code(12);
        let induction = Arm64Register::from_code(3);
        let bound = Arm64Register::from_code(4);
        let polling = Arm64Register::from_code(5);
        let bits = Arm64Register::from_code(6);
        let interrupt_value = Arm64Register::from_code(7);
        let accumulator = Arm64FloatRegister::from_code(2);
        let last_term = Arm64FloatRegister::from_code(3);

        assembler.move_register(state, Arm64Register::X0);
        assembler.move_register(inputs, Arm64Register::X1);
        assembler.move_register(interrupt, Arm64Register::X2);
        assembler.move_register(working_arguments, Arm64Register::from_code(3));
        assembler.load_u64(induction, state, 0);
        assembler.load_u64(bound, state, 8);
        assembler.load_f64(accumulator, state, 16);
        assembler.load_f64(last_term, state, 24);
        assembler.move_immediate(polling, i64::from(SAFEPOINT_INTERVAL));

        assembler.compare_registers(induction, bound);
        let empty_completed =
            assembler.conditional_branch_placeholder(Arm64Condition::GreaterOrEqual);
        let mut side_exits = Vec::new();
        emit_argument_program(
            &mut assembler,
            inputs,
            working_arguments,
            induction,
            bits,
            argument_plan,
            false,
            forwarded_argument_mask,
            &mut side_exits,
        );
        let loop_word = assembler.word_count();
        emit_argument_program(
            &mut assembler,
            inputs,
            working_arguments,
            induction,
            bits,
            argument_plan,
            true,
            forwarded_argument_mask,
            &mut side_exits,
        );
        if let Some(select) = plan.select {
            let (shared_end, true_end, false_end) = select
                .operation_ranges(plan.program.operations.len())
                .expect("validated Double select ranges");
            emit_operations(
                &mut assembler,
                working_arguments,
                bits,
                argument_plan,
                forwarded_argument_mask,
                &plan.program.operations,
                0,
                shared_end,
                &mut side_exits,
            );
            let lhs = emit_source(
                &mut assembler,
                working_arguments,
                bits,
                argument_plan,
                forwarded_argument_mask,
                select.lhs,
                Arm64FloatRegister::from_code(0),
            );
            let rhs = emit_source(
                &mut assembler,
                working_arguments,
                bits,
                argument_plan,
                forwarded_argument_mask,
                select.rhs,
                Arm64FloatRegister::from_code(1),
            );
            assembler.compare_doubles(lhs, rhs);
            let false_condition = match select.kind {
                ScalarLongConditionKind::Equal => Arm64Condition::NotEqual,
                ScalarLongConditionKind::NotEqual => Arm64Condition::Equal,
                ScalarLongConditionKind::LessThan => Arm64Condition::Plus,
                ScalarLongConditionKind::LessThanOrEqual => Arm64Condition::Higher,
            };
            let selected_false = assembler.conditional_branch_placeholder(false_condition);

            emit_operations(
                &mut assembler,
                working_arguments,
                bits,
                argument_plan,
                forwarded_argument_mask,
                &plan.program.operations,
                shared_end,
                true_end,
                &mut side_exits,
            );
            emit_selected_output(
                &mut assembler,
                working_arguments,
                bits,
                argument_plan,
                forwarded_argument_mask,
                select.when_true,
                if select.merge_result {
                    Arm64FloatRegister::from_code(DOUBLE_SELECTION_REGISTER)
                } else {
                    last_term
                },
            );
            let selected_true_join = assembler.branch_placeholder();

            let false_word = assembler.word_count();
            if !assembler.patch_conditional_branch(selected_false, false_word) {
                return Err(QuickDoubleCallAccumulateJitError::BranchOutOfRange);
            }
            emit_operations(
                &mut assembler,
                working_arguments,
                bits,
                argument_plan,
                forwarded_argument_mask,
                &plan.program.operations,
                true_end,
                false_end,
                &mut side_exits,
            );
            emit_selected_output(
                &mut assembler,
                working_arguments,
                bits,
                argument_plan,
                forwarded_argument_mask,
                select.when_false,
                if select.merge_result {
                    Arm64FloatRegister::from_code(DOUBLE_SELECTION_REGISTER)
                } else {
                    last_term
                },
            );
            let continuation_word = assembler.word_count();
            if !assembler.patch_branch(selected_true_join, continuation_word) {
                return Err(QuickDoubleCallAccumulateJitError::BranchOutOfRange);
            }
            if select.merge_result {
                emit_operations(
                    &mut assembler,
                    working_arguments,
                    bits,
                    argument_plan,
                    forwarded_argument_mask,
                    &plan.program.operations,
                    false_end,
                    plan.program.operations.len(),
                    &mut side_exits,
                );
                emit_selected_output(
                    &mut assembler,
                    working_arguments,
                    bits,
                    argument_plan,
                    forwarded_argument_mask,
                    plan.program.output,
                    last_term,
                );
            }
        } else {
            emit_operations(
                &mut assembler,
                working_arguments,
                bits,
                argument_plan,
                forwarded_argument_mask,
                &plan.program.operations,
                0,
                plan.program.operations.len(),
                &mut side_exits,
            );
            emit_selected_output(
                &mut assembler,
                working_arguments,
                bits,
                argument_plan,
                forwarded_argument_mask,
                plan.program.output,
                last_term,
            );
        }
        assembler.add_double(accumulator, accumulator, last_term);
        assembler.add_immediate(induction, induction, 1);
        assembler.compare_registers(induction, bound);
        let active_completed =
            assembler.conditional_branch_placeholder(Arm64Condition::GreaterOrEqual);
        assembler.subtract_immediate(polling, polling, 1);
        assembler.compare_with_zero(polling);
        let hot_backedge = assembler.conditional_branch_placeholder(Arm64Condition::NotEqual);
        assembler.load_u8(interrupt_value, interrupt, 0);
        assembler.compare_with_zero(interrupt_value);
        let interrupted = assembler.conditional_branch_placeholder(Arm64Condition::NotEqual);
        assembler.move_immediate(polling, i64::from(SAFEPOINT_INTERVAL));
        let polled_backedge = assembler.branch_placeholder();

        let completed_word = assembler.word_count();
        emit_publication(&mut assembler, state, induction, accumulator, last_term);
        assembler.move_immediate(Arm64Register::X0, i64::from(STATUS_COMPLETED));
        assembler.ret();

        let interrupted_word = assembler.word_count();
        emit_publication(&mut assembler, state, induction, accumulator, last_term);
        assembler.move_immediate(Arm64Register::X0, i64::from(STATUS_INTERRUPTED));
        assembler.ret();

        let side_exit_word = assembler.word_count();
        emit_publication(&mut assembler, state, induction, accumulator, last_term);
        assembler.move_immediate(Arm64Register::X0, i64::from(STATUS_SIDE_EXIT));
        assembler.ret();

        for (branch, target) in [
            (empty_completed, completed_word),
            (active_completed, completed_word),
            (hot_backedge, loop_word),
            (interrupted, interrupted_word),
        ] {
            if !assembler.patch_conditional_branch(branch, target) {
                return Err(QuickDoubleCallAccumulateJitError::BranchOutOfRange);
            }
        }
        for branch in side_exits {
            if !assembler.patch_conditional_branch(branch, side_exit_word) {
                return Err(QuickDoubleCallAccumulateJitError::BranchOutOfRange);
            }
        }
        if !assembler.patch_branch(polled_backedge, loop_word) {
            return Err(QuickDoubleCallAccumulateJitError::BranchOutOfRange);
        }

        let code = assembler.finish().into_boxed_slice();
        let memory = ExecutableMemory::from_code(&code)?;
        Ok(Self {
            memory,
            code,
            input_count: argument_plan.input_count as usize,
            forwarded_argument_mask,
        })
    }

    pub fn call(
        &self,
        state: &mut NativeDoubleCallAccumulateState,
        inputs: &[f64],
        interrupt: &bool,
    ) -> Result<QuickDoubleCallAccumulateJitOutcome, QuickDoubleCallAccumulateJitError> {
        unsafe { self.call_with_interrupt_ptr(state, inputs, interrupt) }
    }

    unsafe fn call_with_interrupt_ptr(
        &self,
        state: &mut NativeDoubleCallAccumulateState,
        inputs: &[f64],
        interrupt: *const bool,
    ) -> Result<QuickDoubleCallAccumulateJitOutcome, QuickDoubleCallAccumulateJitError> {
        if inputs.len() != self.input_count {
            return Err(QuickDoubleCallAccumulateJitError::InvalidProgram(
                "input count differs from the compiled ABI",
            ));
        }
        type NativeFunction = unsafe extern "C" fn(
            *mut NativeDoubleCallAccumulateState,
            *const f64,
            *const bool,
            *mut f64,
        ) -> u32;
        let function: NativeFunction = unsafe { std::mem::transmute(self.memory.entry()) };
        let mut working_arguments = [0.0_f64; 8];
        match function(
            state,
            inputs.as_ptr(),
            interrupt,
            working_arguments.as_mut_ptr(),
        ) {
            STATUS_COMPLETED => Ok(QuickDoubleCallAccumulateJitOutcome::Completed),
            STATUS_INTERRUPTED => Ok(QuickDoubleCallAccumulateJitOutcome::Interrupted),
            STATUS_SIDE_EXIT => Ok(QuickDoubleCallAccumulateJitOutcome::SideExit),
            status => Err(QuickDoubleCallAccumulateJitError::InvalidNativeStatus(
                status,
            )),
        }
    }

    pub fn code(&self) -> &[u8] {
        &self.code
    }

    pub fn forwarded_argument_mask(&self) -> u8 {
        self.forwarded_argument_mask
    }
}

pub struct QuickDoubleCallAccumulateJitCache {
    target_identities: Cell<[usize; 9]>,
    target_count: Cell<u8>,
    compiled: OnceCell<Option<CompiledQuickDoubleCallAccumulateLoop>>,
    native_entries: Cell<u64>,
    side_exits: Cell<u64>,
}

impl QuickDoubleCallAccumulateJitCache {
    pub const fn new() -> Self {
        Self {
            target_identities: Cell::new([0; 9]),
            target_count: Cell::new(0),
            compiled: OnceCell::new(),
            native_entries: Cell::new(0),
            side_exits: Cell::new(0),
        }
    }

    pub(crate) unsafe fn dispatch(
        &self,
        target_identities: &[usize],
        argument_plan: &QuickDoubleArgumentProgram,
        plan: &ScalarDoubleFunctionPlan,
        state: &mut NativeDoubleCallAccumulateState,
        inputs: &[f64],
        interrupt: *const bool,
    ) -> Option<Result<QuickDoubleCallAccumulateJitOutcome, QuickDoubleCallAccumulateJitError>>
    {
        if target_identities.len() > 9 {
            return None;
        }
        if self.compiled.get().is_none() {
            let mut identities = [0usize; 9];
            identities[..target_identities.len()].copy_from_slice(target_identities);
            self.target_identities.set(identities);
            self.target_count.set(target_identities.len() as u8);
        } else if self.target_count.get() as usize != target_identities.len()
            || self.target_identities.get()[..target_identities.len()] != *target_identities
        {
            return None;
        }
        let program = self
            .compiled
            .get_or_init(|| {
                CompiledQuickDoubleCallAccumulateLoop::compile(argument_plan, plan).ok()
            })
            .as_ref()?;
        self.native_entries
            .set(self.native_entries.get().saturating_add(1));
        let outcome = program.call_with_interrupt_ptr(state, inputs, interrupt);
        if matches!(
            outcome,
            Ok(QuickDoubleCallAccumulateJitOutcome::SideExit) | Err(_)
        ) {
            self.side_exits.set(self.side_exits.get().saturating_add(1));
        }
        Some(outcome)
    }

    pub fn is_compiled(&self) -> bool {
        matches!(self.compiled.get(), Some(Some(_)))
    }

    pub fn native_entries(&self) -> u64 {
        self.native_entries.get()
    }

    pub fn side_exits(&self) -> u64 {
        self.side_exits.get()
    }
}

impl Default for QuickDoubleCallAccumulateJitCache {
    fn default() -> Self {
        Self::new()
    }
}

impl Clone for QuickDoubleCallAccumulateJitCache {
    fn clone(&self) -> Self {
        Self::new()
    }
}

impl fmt::Debug for QuickDoubleCallAccumulateJitCache {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter
            .debug_struct("QuickDoubleCallAccumulateJitCache")
            .field("compiled", &self.is_compiled())
            .field("native_entries", &self.native_entries())
            .field("side_exits", &self.side_exits())
            .finish()
    }
}

fn validate(plan: &ScalarDoubleFunctionPlan) -> Result<(), QuickDoubleCallAccumulateJitError> {
    plan.validate_register_program(MAX_INPUTS, MAX_OPERATIONS)
        .map_err(QuickDoubleCallAccumulateJitError::InvalidProgram)
}

fn validate_argument_plan(
    plan: &QuickDoubleArgumentProgram,
    public_args: u8,
) -> Result<(), QuickDoubleCallAccumulateJitError> {
    if plan.operations.len() > MAX_OPERATIONS
        || plan.output_count != public_args
        || plan.output_count as usize > plan.outputs.len()
        || plan.input_count as usize > plan.input_slots.len()
    {
        return Err(QuickDoubleCallAccumulateJitError::InvalidProgram(
            "argument program exceeds the native ABI",
        ));
    }
    for (index, operation) in plan.operations.iter().enumerate() {
        validate_argument_source(operation.lhs, index, plan.input_count)?;
        validate_argument_source(operation.rhs, index, plan.input_count)?;
    }
    for output in plan.outputs[..plan.output_count as usize].iter().copied() {
        validate_argument_source(output, plan.operations.len(), plan.input_count)?;
    }
    Ok(())
}

fn validate_argument_source(
    source: QuickDoubleSource,
    available_temporaries: usize,
    input_count: u8,
) -> Result<(), QuickDoubleCallAccumulateJitError> {
    match source {
        QuickDoubleSource::Input(index) if index >= input_count => Err(
            QuickDoubleCallAccumulateJitError::InvalidProgram("argument input is outside the ABI"),
        ),
        QuickDoubleSource::Temporary(index) if index as usize >= available_temporaries => {
            Err(QuickDoubleCallAccumulateJitError::InvalidProgram(
                "argument temporary is used before definition",
            ))
        }
        _ => Ok(()),
    }
}

fn emit_argument_program(
    assembler: &mut Arm64Assembler,
    inputs: Arm64Register,
    outputs: Arm64Register,
    induction: Arm64Register,
    bits: Arm64Register,
    plan: &QuickDoubleArgumentProgram,
    induction_dependent: bool,
    forwarded_argument_mask: u8,
    side_exits: &mut Vec<usize>,
) {
    for (index, operation) in plan.operations.iter().copied().enumerate() {
        if !plan.operation_is_needed_by_output_phase(index, induction_dependent) {
            continue;
        }
        emit_argument_operation(
            assembler, inputs, induction, bits, index, operation, side_exits,
        );
    }
    for (index, output) in plan.outputs[..plan.output_count as usize]
        .iter()
        .copied()
        .enumerate()
    {
        if plan.source_depends_on_induction(output) != induction_dependent {
            continue;
        }
        if forwarded_argument_mask & (1_u8 << index) != 0 {
            continue;
        }
        let output = emit_argument_source(
            assembler,
            inputs,
            induction,
            bits,
            output,
            Arm64FloatRegister::from_code(0),
        );
        assembler.store_f64(output, outputs, index as u16 * 8);
    }
}

fn emit_argument_operation(
    assembler: &mut Arm64Assembler,
    inputs: Arm64Register,
    induction: Arm64Register,
    bits: Arm64Register,
    index: usize,
    operation: QuickDoubleArgumentOp,
    side_exits: &mut Vec<usize>,
) {
    let lhs = emit_argument_source(
        assembler,
        inputs,
        induction,
        bits,
        operation.lhs,
        Arm64FloatRegister::from_code(0),
    );
    let rhs = emit_argument_source(
        assembler,
        inputs,
        induction,
        bits,
        operation.rhs,
        Arm64FloatRegister::from_code(1),
    );
    let destination = temporary(index);
    match operation.kind {
        ScalarDoubleOpKind::Add => assembler.add_double(destination, lhs, rhs),
        ScalarDoubleOpKind::Subtract => assembler.subtract_double(destination, lhs, rhs),
        ScalarDoubleOpKind::Multiply => assembler.multiply_double(destination, lhs, rhs),
        ScalarDoubleOpKind::Divide => {
            assembler.compare_double_with_zero(rhs);
            side_exits.push(assembler.conditional_branch_placeholder(Arm64Condition::Equal));
            assembler.divide_double(destination, lhs, rhs);
        }
    }
}

fn emit_argument_source(
    assembler: &mut Arm64Assembler,
    inputs: Arm64Register,
    induction: Arm64Register,
    bits: Arm64Register,
    source: QuickDoubleSource,
    scratch: Arm64FloatRegister,
) -> Arm64FloatRegister {
    match source {
        QuickDoubleSource::Input(index) => {
            assembler.load_f64(scratch, inputs, u16::from(index) * 8);
            scratch
        }
        QuickDoubleSource::Induction => {
            assembler.convert_signed_to_double(scratch, induction);
            scratch
        }
        QuickDoubleSource::Constant(value) => {
            assembler.move_immediate(bits, value.to_bits() as i64);
            assembler.move_register_bits_to_double(scratch, bits);
            scratch
        }
        QuickDoubleSource::Temporary(index) => temporary(index as usize),
    }
}

fn emit_operation(
    assembler: &mut Arm64Assembler,
    inputs: Arm64Register,
    bits: Arm64Register,
    argument_plan: &QuickDoubleArgumentProgram,
    forwarded_argument_mask: u8,
    index: usize,
    operation: ScalarDoubleOp,
    side_exits: &mut Vec<usize>,
) {
    let lhs = emit_source(
        assembler,
        inputs,
        bits,
        argument_plan,
        forwarded_argument_mask,
        operation.lhs,
        Arm64FloatRegister::from_code(0),
    );
    let rhs = emit_source(
        assembler,
        inputs,
        bits,
        argument_plan,
        forwarded_argument_mask,
        operation.rhs,
        Arm64FloatRegister::from_code(1),
    );
    let destination = temporary(index);
    match operation.kind {
        ScalarDoubleOpKind::Add => assembler.add_double(destination, lhs, rhs),
        ScalarDoubleOpKind::Subtract => assembler.subtract_double(destination, lhs, rhs),
        ScalarDoubleOpKind::Multiply => assembler.multiply_double(destination, lhs, rhs),
        ScalarDoubleOpKind::Divide => {
            assembler.compare_double_with_zero(rhs);
            side_exits.push(assembler.conditional_branch_placeholder(Arm64Condition::Equal));
            assembler.divide_double(destination, lhs, rhs);
        }
    }
}

#[allow(clippy::too_many_arguments)]
fn emit_operations(
    assembler: &mut Arm64Assembler,
    inputs: Arm64Register,
    bits: Arm64Register,
    argument_plan: &QuickDoubleArgumentProgram,
    forwarded_argument_mask: u8,
    operations: &[ScalarDoubleOp],
    start: usize,
    end: usize,
    side_exits: &mut Vec<usize>,
) {
    for (relative_index, operation) in operations[start..end].iter().copied().enumerate() {
        emit_operation(
            assembler,
            inputs,
            bits,
            argument_plan,
            forwarded_argument_mask,
            start + relative_index,
            operation,
            side_exits,
        );
    }
}

#[allow(clippy::too_many_arguments)]
fn emit_selected_output(
    assembler: &mut Arm64Assembler,
    inputs: Arm64Register,
    bits: Arm64Register,
    argument_plan: &QuickDoubleArgumentProgram,
    forwarded_argument_mask: u8,
    source: ScalarDoubleSource,
    destination: Arm64FloatRegister,
) {
    let output = emit_source(
        assembler,
        inputs,
        bits,
        argument_plan,
        forwarded_argument_mask,
        source,
        Arm64FloatRegister::from_code(0),
    );
    assembler.move_double(destination, output);
}

fn emit_source(
    assembler: &mut Arm64Assembler,
    inputs: Arm64Register,
    bits: Arm64Register,
    argument_plan: &QuickDoubleArgumentProgram,
    forwarded_argument_mask: u8,
    source: ScalarDoubleSource,
    scratch: Arm64FloatRegister,
) -> Arm64FloatRegister {
    match source {
        ScalarDoubleSource::Input(index) if forwarded_argument_mask & (1_u8 << index) != 0 => {
            let QuickDoubleSource::Temporary(index) = argument_plan.outputs[index as usize] else {
                unreachable!("forwarded Double argument must be a temporary")
            };
            temporary(index as usize)
        }
        ScalarDoubleSource::Input(index) => {
            assembler.load_f64(scratch, inputs, index * 8);
            scratch
        }
        ScalarDoubleSource::Constant(value) => {
            assembler.move_immediate(bits, value.to_bits() as i64);
            assembler.move_register_bits_to_double(scratch, bits);
            scratch
        }
        ScalarDoubleSource::Temporary(index) => temporary(index as usize),
        ScalarDoubleSource::Selection => Arm64FloatRegister::from_code(DOUBLE_SELECTION_REGISTER),
    }
}

#[inline]
fn temporary(index: usize) -> Arm64FloatRegister {
    Arm64FloatRegister::from_code(FIRST_TEMPORARY + index as u8)
}

fn emit_publication(
    assembler: &mut Arm64Assembler,
    state: Arm64Register,
    induction: Arm64Register,
    accumulator: Arm64FloatRegister,
    last_term: Arm64FloatRegister,
) {
    assembler.store_u64(induction, state, 0);
    assembler.store_f64(accumulator, state, 16);
    assembler.store_f64(last_term, state, 24);
}
