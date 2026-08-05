//! Composed exact-Double call/accumulate loop lowering for macOS ARM64.

use super::super::memory::ExecutableMemory;
use super::{Arm64Assembler, Arm64Condition, Arm64FloatRegister, Arm64Register};
use crate::vm::function::{
    ScalarDoubleFunctionPlan, ScalarDoubleOp, ScalarDoubleOpKind, ScalarDoubleSource,
};
use std::cell::{Cell, OnceCell};
use std::fmt;
use std::io;

const MAX_INPUTS: usize = 8;
const MAX_OPERATIONS: usize = 8;
const FIRST_TEMPORARY: u8 = 16;
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

/// Native ABI: x0 is state, x1 exact-Double inputs, x2 interrupt flag; w0 is
/// the outcome. A side exit publishes only iterations completed before the
/// failing division so the VM can restart at `InitFcall` exactly.
pub struct CompiledQuickDoubleCallAccumulateLoop {
    memory: ExecutableMemory,
    code: Box<[u8]>,
    input_count: usize,
}

impl CompiledQuickDoubleCallAccumulateLoop {
    pub fn compile(
        plan: &ScalarDoubleFunctionPlan,
    ) -> Result<Self, QuickDoubleCallAccumulateJitError> {
        validate(plan)?;

        let mut assembler = Arm64Assembler::new();
        let state = Arm64Register::from_code(9);
        let inputs = Arm64Register::from_code(10);
        let interrupt = Arm64Register::from_code(11);
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
        assembler.load_u64(induction, state, 0);
        assembler.load_u64(bound, state, 8);
        assembler.load_f64(accumulator, state, 16);
        assembler.load_f64(last_term, state, 24);
        assembler.move_immediate(polling, i64::from(SAFEPOINT_INTERVAL));

        assembler.compare_registers(induction, bound);
        let empty_completed =
            assembler.conditional_branch_placeholder(Arm64Condition::GreaterOrEqual);
        let loop_word = assembler.word_count();
        let mut side_exits = Vec::new();
        for (index, operation) in plan.program.operations.iter().copied().enumerate() {
            emit_operation(
                &mut assembler,
                inputs,
                bits,
                index,
                operation,
                &mut side_exits,
            );
        }
        let output = emit_source(
            &mut assembler,
            inputs,
            bits,
            plan.program.output,
            Arm64FloatRegister::from_code(0),
        );
        assembler.move_double(last_term, output);
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
            input_count: plan.public_args as usize,
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
        ) -> u32;
        let function: NativeFunction = unsafe { std::mem::transmute(self.memory.entry()) };
        match function(state, inputs.as_ptr(), interrupt) {
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
}

pub struct QuickDoubleCallAccumulateJitCache {
    target_identity: Cell<usize>,
    compiled: OnceCell<Option<CompiledQuickDoubleCallAccumulateLoop>>,
    native_entries: Cell<u64>,
    side_exits: Cell<u64>,
}

impl QuickDoubleCallAccumulateJitCache {
    pub const fn new() -> Self {
        Self {
            target_identity: Cell::new(0),
            compiled: OnceCell::new(),
            native_entries: Cell::new(0),
            side_exits: Cell::new(0),
        }
    }

    pub(crate) unsafe fn dispatch(
        &self,
        target_identity: usize,
        plan: &ScalarDoubleFunctionPlan,
        state: &mut NativeDoubleCallAccumulateState,
        inputs: &[f64],
        interrupt: *const bool,
    ) -> Option<Result<QuickDoubleCallAccumulateJitOutcome, QuickDoubleCallAccumulateJitError>>
    {
        if self.compiled.get().is_none() {
            self.target_identity.set(target_identity);
        } else if self.target_identity.get() != target_identity {
            return None;
        }
        let program = self
            .compiled
            .get_or_init(|| CompiledQuickDoubleCallAccumulateLoop::compile(plan).ok())
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
    if plan.public_args as usize > MAX_INPUTS || plan.program.operations.len() > MAX_OPERATIONS {
        return Err(QuickDoubleCallAccumulateJitError::InvalidProgram(
            "program exceeds the register ABI",
        ));
    }
    for (index, operation) in plan.program.operations.iter().enumerate() {
        validate_source(operation.lhs, index, plan.public_args)?;
        validate_source(operation.rhs, index, plan.public_args)?;
    }
    validate_source(
        plan.program.output,
        plan.program.operations.len(),
        plan.public_args,
    )
}

fn validate_source(
    source: ScalarDoubleSource,
    available_temporaries: usize,
    inputs: u8,
) -> Result<(), QuickDoubleCallAccumulateJitError> {
    match source {
        ScalarDoubleSource::Input(index) if index >= u16::from(inputs) => Err(
            QuickDoubleCallAccumulateJitError::InvalidProgram("input is outside the public ABI"),
        ),
        ScalarDoubleSource::Temporary(index) if index as usize >= available_temporaries => {
            Err(QuickDoubleCallAccumulateJitError::InvalidProgram(
                "temporary is used before definition",
            ))
        }
        _ => Ok(()),
    }
}

fn emit_operation(
    assembler: &mut Arm64Assembler,
    inputs: Arm64Register,
    bits: Arm64Register,
    index: usize,
    operation: ScalarDoubleOp,
    side_exits: &mut Vec<usize>,
) {
    let lhs = emit_source(
        assembler,
        inputs,
        bits,
        operation.lhs,
        Arm64FloatRegister::from_code(0),
    );
    let rhs = emit_source(
        assembler,
        inputs,
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

fn emit_source(
    assembler: &mut Arm64Assembler,
    inputs: Arm64Register,
    bits: Arm64Register,
    source: ScalarDoubleSource,
    scratch: Arm64FloatRegister,
) -> Arm64FloatRegister {
    match source {
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
