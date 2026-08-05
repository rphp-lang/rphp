//! Composed exact-Double call/accumulate loop lowering for Linux x86-64.

use super::super::memory::ExecutableMemory;
use super::{X86_64Assembler, X86_64FloatRegister, X86_64Register};
use crate::vm::function::{
    ScalarDoubleFunctionPlan, ScalarDoubleOp, ScalarDoubleOpKind, ScalarDoubleSource,
};
use crate::vm::quick::{
    QuickDoubleArgumentOp, QuickDoubleArgumentProgram, QuickDoubleSource,
};
use std::cell::{Cell, OnceCell};
use std::fmt;
use std::io;

const MAX_INPUTS: usize = 8;
const MAX_OPERATIONS: usize = 8;
const FIRST_TEMPORARY: u8 = 2;
const SAFEPOINT_INTERVAL: i64 = 1024;
const STATUS_COMPLETED: u32 = 0;
const STATUS_INTERRUPTED: u32 = 1;
const STATUS_SIDE_EXIT: u32 = 2;

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
    Memory(io::Error),
    InvalidNativeStatus(u32),
}

impl fmt::Display for QuickDoubleCallAccumulateJitError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::InvalidProgram(reason) => write!(formatter, "invalid Double loop: {reason}"),
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

/// SysV ABI: RDI is state, RSI compact exact-Double inputs, RDX is the
/// interrupt flag and RCX is the writable argument buffer; EAX returns the
/// outcome. XMM2-XMM9 hold the target-neutral IR temporaries, while XMM10 and
/// XMM11 keep accumulator and last committed term resident.
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

        let mut assembler = X86_64Assembler::new();
        let state = X86_64Register::R10;
        let inputs = X86_64Register::R11;
        let interrupt = X86_64Register::R9;
        let working_arguments = X86_64Register::RDI;
        let induction = X86_64Register::RCX;
        let bound = X86_64Register::R8;
        let polling = X86_64Register::RDX;
        let bits = X86_64Register::RAX;
        let accumulator = X86_64FloatRegister::from_code(10);
        let last_term = X86_64FloatRegister::from_code(11);

        assembler.move_register(state, X86_64Register::RDI);
        assembler.move_register(inputs, X86_64Register::RSI);
        assembler.move_register(interrupt, X86_64Register::RDX);
        assembler.move_register(working_arguments, X86_64Register::RCX);
        assembler.move_from_base_disp32(induction, state, 0);
        assembler.move_from_base_disp32(bound, state, 8);
        assembler.load_f64(accumulator, state, 16);
        assembler.load_f64(last_term, state, 24);
        assembler.move_immediate64(polling, SAFEPOINT_INTERVAL);

        assembler.compare_register(induction, bound);
        let empty_completed = assembler.jump_greater_or_equal_rel32();
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
        let loop_start = assembler.bytes.len();
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
        for (index, operation) in plan.program.operations.iter().copied().enumerate() {
            emit_operation(
                &mut assembler,
                working_arguments,
                bits,
                argument_plan,
                forwarded_argument_mask,
                index,
                operation,
                &mut side_exits,
            );
        }
        let output = emit_source(
            &mut assembler,
            working_arguments,
            bits,
            argument_plan,
            forwarded_argument_mask,
            plan.program.output,
            X86_64FloatRegister::from_code(0),
        );
        assembler.move_double(last_term, output);
        assembler.add_double(accumulator, last_term);
        assembler.add_immediate8(induction, 1);
        assembler.compare_register(induction, bound);
        let active_completed = assembler.jump_greater_or_equal_rel32();
        assembler.subtract_immediate8(polling, 1);
        assembler.compare_immediate8(polling, 0);
        let hot_backedge = assembler.jump_not_equal_rel32();
        assembler.compare_byte_base_immediate8(interrupt, 0);
        let interrupted = assembler.jump_not_equal_rel32();
        assembler.move_immediate64(polling, SAFEPOINT_INTERVAL);
        let polled_backedge = assembler.jump_rel32();

        let completed = assembler.bytes.len();
        emit_publication(&mut assembler, state, induction, accumulator, last_term);
        assembler.move_immediate32_eax(STATUS_COMPLETED);
        assembler.return_near();

        let interrupted_target = assembler.bytes.len();
        emit_publication(&mut assembler, state, induction, accumulator, last_term);
        assembler.move_immediate32_eax(STATUS_INTERRUPTED);
        assembler.return_near();

        let side_exit = assembler.bytes.len();
        emit_publication(&mut assembler, state, induction, accumulator, last_term);
        assembler.move_immediate32_eax(STATUS_SIDE_EXIT);
        assembler.return_near();

        for jump in [empty_completed, active_completed] {
            assembler.patch_rel32(jump, completed);
        }
        assembler.patch_rel32(hot_backedge, loop_start);
        assembler.patch_rel32(polled_backedge, loop_start);
        assembler.patch_rel32(interrupted, interrupted_target);
        for jump in side_exits {
            assembler.patch_rel32(jump, side_exit);
        }

        let code = assembler.finish();
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
        argument_plan: &QuickDoubleArgumentProgram,
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
        QuickDoubleSource::Temporary(index) if index as usize >= available_temporaries => Err(
            QuickDoubleCallAccumulateJitError::InvalidProgram(
                "argument temporary is used before definition",
            ),
        ),
        _ => Ok(()),
    }
}

fn emit_argument_program(
    assembler: &mut X86_64Assembler,
    inputs: X86_64Register,
    outputs: X86_64Register,
    induction: X86_64Register,
    bits: X86_64Register,
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
            assembler,
            inputs,
            induction,
            bits,
            index,
            operation,
            side_exits,
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
            X86_64FloatRegister::from_code(0),
        );
        assembler.store_f64(outputs, output, index as i32 * 8);
    }
}

fn emit_argument_operation(
    assembler: &mut X86_64Assembler,
    inputs: X86_64Register,
    induction: X86_64Register,
    bits: X86_64Register,
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
        X86_64FloatRegister::from_code(0),
    );
    let rhs = emit_argument_source(
        assembler,
        inputs,
        induction,
        bits,
        operation.rhs,
        X86_64FloatRegister::from_code(1),
    );
    let destination = temporary(index);
    assembler.move_double(destination, lhs);
    match operation.kind {
        ScalarDoubleOpKind::Add => assembler.add_double(destination, rhs),
        ScalarDoubleOpKind::Subtract => assembler.subtract_double(destination, rhs),
        ScalarDoubleOpKind::Multiply => assembler.multiply_double(destination, rhs),
        ScalarDoubleOpKind::Divide => {
            assembler.move_double_bits_to_gpr(bits, rhs);
            assembler.shift_left_immediate8(bits, 1);
            assembler.compare_immediate8(bits, 0);
            side_exits.push(assembler.jump_equal_rel32());
            assembler.divide_double(destination, rhs);
        }
    }
}

fn emit_argument_source(
    assembler: &mut X86_64Assembler,
    inputs: X86_64Register,
    induction: X86_64Register,
    bits: X86_64Register,
    source: QuickDoubleSource,
    scratch: X86_64FloatRegister,
) -> X86_64FloatRegister {
    match source {
        QuickDoubleSource::Input(index) => {
            assembler.load_f64(scratch, inputs, i32::from(index) * 8);
            scratch
        }
        QuickDoubleSource::Induction => {
            assembler.convert_signed_to_double(scratch, induction);
            scratch
        }
        QuickDoubleSource::Constant(value) => {
            assembler.move_immediate64(bits, value.to_bits() as i64);
            assembler.move_gpr_bits_to_double(scratch, bits);
            scratch
        }
        QuickDoubleSource::Temporary(index) => temporary(index as usize),
    }
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
    assembler: &mut X86_64Assembler,
    inputs: X86_64Register,
    bits: X86_64Register,
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
        X86_64FloatRegister::from_code(0),
    );
    let rhs = emit_source(
        assembler,
        inputs,
        bits,
        argument_plan,
        forwarded_argument_mask,
        operation.rhs,
        X86_64FloatRegister::from_code(1),
    );
    let destination = temporary(index);
    assembler.move_double(destination, lhs);
    match operation.kind {
        ScalarDoubleOpKind::Add => assembler.add_double(destination, rhs),
        ScalarDoubleOpKind::Subtract => assembler.subtract_double(destination, rhs),
        ScalarDoubleOpKind::Multiply => assembler.multiply_double(destination, rhs),
        ScalarDoubleOpKind::Divide => {
            assembler.move_double_bits_to_gpr(bits, rhs);
            assembler.shift_left_immediate8(bits, 1);
            assembler.compare_immediate8(bits, 0);
            side_exits.push(assembler.jump_equal_rel32());
            assembler.divide_double(destination, rhs);
        }
    }
}

fn emit_source(
    assembler: &mut X86_64Assembler,
    inputs: X86_64Register,
    bits: X86_64Register,
    argument_plan: &QuickDoubleArgumentProgram,
    forwarded_argument_mask: u8,
    source: ScalarDoubleSource,
    scratch: X86_64FloatRegister,
) -> X86_64FloatRegister {
    match source {
        ScalarDoubleSource::Input(index) if forwarded_argument_mask & (1_u8 << index) != 0 => {
            let QuickDoubleSource::Temporary(index) = argument_plan.outputs[index as usize] else {
                unreachable!("forwarded Double argument must be a temporary")
            };
            temporary(index as usize)
        }
        ScalarDoubleSource::Input(index) => {
            assembler.load_f64(scratch, inputs, i32::from(index) * 8);
            scratch
        }
        ScalarDoubleSource::Constant(value) => {
            assembler.move_immediate64(bits, value.to_bits() as i64);
            assembler.move_gpr_bits_to_double(scratch, bits);
            scratch
        }
        ScalarDoubleSource::Temporary(index) => temporary(index as usize),
    }
}

#[inline]
fn temporary(index: usize) -> X86_64FloatRegister {
    X86_64FloatRegister::from_code(FIRST_TEMPORARY + index as u8)
}

fn emit_publication(
    assembler: &mut X86_64Assembler,
    state: X86_64Register,
    induction: X86_64Register,
    accumulator: X86_64FloatRegister,
    last_term: X86_64FloatRegister,
) {
    assembler.move_to_base_disp32(state, induction, 0);
    assembler.store_f64(state, accumulator, 16);
    assembler.store_f64(state, last_term, 24);
}
