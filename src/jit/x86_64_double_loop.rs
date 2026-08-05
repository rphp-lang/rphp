//! Composed exact-Double call/accumulate loop lowering for Linux x86-64.

use super::super::memory::ExecutableMemory;
use super::double::X86ScalarDoubleRegisterMap;
use super::{X86_64Assembler, X86_64FloatRegister, X86_64Register, X86DoubleInstructionSet};
use crate::vm::function::{
    ScalarDoubleFunctionPlan, ScalarDoubleOp, ScalarDoubleOpKind, ScalarDoubleSource,
    ScalarLongConditionKind,
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
const DOUBLE_SELECTION_REGISTER: u8 = 13;
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
/// outcome. XMM2-XMM9 form the target-neutral temporary register bank, while
/// XMM10 and XMM11 keep accumulator and last committed term resident.
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
        Self::compile_with_instruction_set(argument_plan, plan, X86DoubleInstructionSet::detected())
    }

    pub(super) fn compile_with_instruction_set(
        argument_plan: &QuickDoubleArgumentProgram,
        plan: &ScalarDoubleFunctionPlan,
        instruction_set: X86DoubleInstructionSet,
    ) -> Result<Self, QuickDoubleCallAccumulateJitError> {
        validate_argument_plan(argument_plan, plan.public_args)?;
        validate(plan)?;
        let forwarded_argument_mask = argument_plan.register_forwardable_output_mask(plan);
        let scalar_registers = X86ScalarDoubleRegisterMap::for_plan(plan);

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
        let avx_upper_zero = X86_64FloatRegister::from_code(12);

        assembler.move_register(state, X86_64Register::RDI);
        assembler.move_register(inputs, X86_64Register::RSI);
        assembler.move_register(interrupt, X86_64Register::RDX);
        assembler.move_register(working_arguments, X86_64Register::RCX);
        assembler.move_from_base_disp32(induction, state, 0);
        assembler.move_from_base_disp32(bound, state, 8);
        emit_load_f64(&mut assembler, instruction_set, accumulator, state, 16);
        emit_load_f64(&mut assembler, instruction_set, last_term, state, 24);
        if instruction_set == X86DoubleInstructionSet::Avx {
            assembler.zero_double_register_avx(avx_upper_zero);
        }
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
            avx_upper_zero,
            instruction_set,
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
            avx_upper_zero,
            instruction_set,
            argument_plan,
            true,
            forwarded_argument_mask,
            &mut side_exits,
        );
        if let Some(select) = plan.select {
            let (shared_end, true_end, false_end) = select
                .operation_ranges(plan.program.operations.len())
                .expect("validated Double select must have valid operation ranges");
            emit_operations(
                &mut assembler,
                working_arguments,
                bits,
                argument_plan,
                forwarded_argument_mask,
                scalar_registers,
                instruction_set,
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
                scalar_registers,
                instruction_set,
                select.lhs,
                X86_64FloatRegister::from_code(0),
            );
            let rhs = emit_source(
                &mut assembler,
                working_arguments,
                bits,
                argument_plan,
                forwarded_argument_mask,
                scalar_registers,
                instruction_set,
                select.rhs,
                X86_64FloatRegister::from_code(1),
            );
            match instruction_set {
                X86DoubleInstructionSet::Sse2 => assembler.compare_doubles(lhs, rhs),
                X86DoubleInstructionSet::Avx => assembler.compare_doubles_avx(lhs, rhs),
            }
            let mut selected_false = Vec::with_capacity(2);
            let mut selected_true = None;
            match select.kind {
                ScalarLongConditionKind::Equal => {
                    selected_false.push(assembler.jump_parity_rel32());
                    selected_false.push(assembler.jump_not_equal_rel32());
                }
                ScalarLongConditionKind::NotEqual => {
                    selected_true = Some(assembler.jump_parity_rel32());
                    selected_false.push(assembler.jump_equal_rel32());
                }
                ScalarLongConditionKind::LessThan => {
                    selected_false.push(assembler.jump_parity_rel32());
                    selected_false.push(assembler.jump_above_or_equal_rel32());
                }
                ScalarLongConditionKind::LessThanOrEqual => {
                    selected_false.push(assembler.jump_parity_rel32());
                    selected_false.push(assembler.jump_above_rel32());
                }
            }
            let true_offset = assembler.bytes.len();
            if let Some(jump) = selected_true {
                assembler.patch_rel32(jump, true_offset);
            }
            emit_operations(
                &mut assembler,
                working_arguments,
                bits,
                argument_plan,
                forwarded_argument_mask,
                scalar_registers,
                instruction_set,
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
                scalar_registers,
                instruction_set,
                select.when_true,
                if select.merge_result {
                    X86_64FloatRegister::from_code(DOUBLE_SELECTION_REGISTER)
                } else {
                    last_term
                },
            );
            let selected_true_join = assembler.jump_rel32();

            let false_offset = assembler.bytes.len();
            for jump in selected_false {
                assembler.patch_rel32(jump, false_offset);
            }
            emit_operations(
                &mut assembler,
                working_arguments,
                bits,
                argument_plan,
                forwarded_argument_mask,
                scalar_registers,
                instruction_set,
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
                scalar_registers,
                instruction_set,
                select.when_false,
                if select.merge_result {
                    X86_64FloatRegister::from_code(DOUBLE_SELECTION_REGISTER)
                } else {
                    last_term
                },
            );
            let continuation = assembler.bytes.len();
            assembler.patch_rel32(selected_true_join, continuation);
            if select.merge_result {
                emit_operations(
                    &mut assembler,
                    working_arguments,
                    bits,
                    argument_plan,
                    forwarded_argument_mask,
                    scalar_registers,
                    instruction_set,
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
                    scalar_registers,
                    instruction_set,
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
                scalar_registers,
                instruction_set,
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
                scalar_registers,
                instruction_set,
                plan.program.output,
                last_term,
            );
        }
        match instruction_set {
            X86DoubleInstructionSet::Sse2 => assembler.add_double(accumulator, last_term),
            X86DoubleInstructionSet::Avx => {
                assembler.add_double_avx(accumulator, accumulator, last_term)
            }
        }
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
        emit_publication(
            &mut assembler,
            instruction_set,
            state,
            induction,
            accumulator,
            last_term,
        );
        if instruction_set == X86DoubleInstructionSet::Avx {
            assembler.vzeroupper();
        }
        assembler.move_immediate32_eax(STATUS_COMPLETED);
        assembler.return_near();

        let interrupted_target = assembler.bytes.len();
        emit_publication(
            &mut assembler,
            instruction_set,
            state,
            induction,
            accumulator,
            last_term,
        );
        if instruction_set == X86DoubleInstructionSet::Avx {
            assembler.vzeroupper();
        }
        assembler.move_immediate32_eax(STATUS_INTERRUPTED);
        assembler.return_near();

        let side_exit = assembler.bytes.len();
        emit_publication(
            &mut assembler,
            instruction_set,
            state,
            induction,
            accumulator,
            last_term,
        );
        if instruction_set == X86DoubleInstructionSet::Avx {
            assembler.vzeroupper();
        }
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
    avx_upper_zero: X86_64FloatRegister,
    instruction_set: X86DoubleInstructionSet,
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
            avx_upper_zero,
            instruction_set,
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
            avx_upper_zero,
            instruction_set,
            output,
            X86_64FloatRegister::from_code(0),
        );
        emit_store_f64(
            assembler,
            instruction_set,
            outputs,
            output,
            index as i32 * 8,
        );
    }
}

fn emit_argument_operation(
    assembler: &mut X86_64Assembler,
    inputs: X86_64Register,
    induction: X86_64Register,
    bits: X86_64Register,
    avx_upper_zero: X86_64FloatRegister,
    instruction_set: X86DoubleInstructionSet,
    index: usize,
    operation: QuickDoubleArgumentOp,
    side_exits: &mut Vec<usize>,
) {
    let lhs = emit_argument_source(
        assembler,
        inputs,
        induction,
        bits,
        avx_upper_zero,
        instruction_set,
        operation.lhs,
        X86_64FloatRegister::from_code(0),
    );
    let rhs = emit_argument_source(
        assembler,
        inputs,
        induction,
        bits,
        avx_upper_zero,
        instruction_set,
        operation.rhs,
        X86_64FloatRegister::from_code(1),
    );
    let destination = argument_temporary(index);
    emit_double_operation(
        assembler,
        instruction_set,
        bits,
        destination,
        lhs,
        rhs,
        operation.kind,
        side_exits,
    );
}

fn emit_argument_source(
    assembler: &mut X86_64Assembler,
    inputs: X86_64Register,
    induction: X86_64Register,
    bits: X86_64Register,
    avx_upper_zero: X86_64FloatRegister,
    instruction_set: X86DoubleInstructionSet,
    source: QuickDoubleSource,
    scratch: X86_64FloatRegister,
) -> X86_64FloatRegister {
    match source {
        QuickDoubleSource::Input(index) => {
            emit_load_f64(
                assembler,
                instruction_set,
                scratch,
                inputs,
                i32::from(index) * 8,
            );
            scratch
        }
        QuickDoubleSource::Induction => {
            match instruction_set {
                X86DoubleInstructionSet::Sse2 => {
                    assembler.convert_signed_to_double(scratch, induction)
                }
                X86DoubleInstructionSet::Avx => {
                    assembler.convert_signed_to_double_avx(scratch, avx_upper_zero, induction)
                }
            }
            scratch
        }
        QuickDoubleSource::Constant(value) => {
            assembler.move_immediate64(bits, value.to_bits() as i64);
            emit_gpr_bits_to_double(assembler, instruction_set, scratch, bits);
            scratch
        }
        QuickDoubleSource::Temporary(index) => argument_temporary(index as usize),
    }
}

fn emit_operation(
    assembler: &mut X86_64Assembler,
    inputs: X86_64Register,
    bits: X86_64Register,
    argument_plan: &QuickDoubleArgumentProgram,
    forwarded_argument_mask: u8,
    scalar_registers: X86ScalarDoubleRegisterMap,
    instruction_set: X86DoubleInstructionSet,
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
        scalar_registers,
        instruction_set,
        operation.lhs,
        X86_64FloatRegister::from_code(0),
    );
    let rhs = emit_source(
        assembler,
        inputs,
        bits,
        argument_plan,
        forwarded_argument_mask,
        scalar_registers,
        instruction_set,
        operation.rhs,
        X86_64FloatRegister::from_code(1),
    );
    let destination = scalar_registers.temporary(index);
    emit_double_operation(
        assembler,
        instruction_set,
        bits,
        destination,
        lhs,
        rhs,
        operation.kind,
        side_exits,
    );
}

#[allow(clippy::too_many_arguments)]
fn emit_operations(
    assembler: &mut X86_64Assembler,
    inputs: X86_64Register,
    bits: X86_64Register,
    argument_plan: &QuickDoubleArgumentProgram,
    forwarded_argument_mask: u8,
    scalar_registers: X86ScalarDoubleRegisterMap,
    instruction_set: X86DoubleInstructionSet,
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
            scalar_registers,
            instruction_set,
            start + relative_index,
            operation,
            side_exits,
        );
    }
}

#[allow(clippy::too_many_arguments)]
fn emit_selected_output(
    assembler: &mut X86_64Assembler,
    inputs: X86_64Register,
    bits: X86_64Register,
    argument_plan: &QuickDoubleArgumentProgram,
    forwarded_argument_mask: u8,
    scalar_registers: X86ScalarDoubleRegisterMap,
    instruction_set: X86DoubleInstructionSet,
    source: ScalarDoubleSource,
    destination: X86_64FloatRegister,
) {
    let output = emit_source(
        assembler,
        inputs,
        bits,
        argument_plan,
        forwarded_argument_mask,
        scalar_registers,
        instruction_set,
        source,
        X86_64FloatRegister::from_code(0),
    );
    emit_move_double(assembler, instruction_set, destination, output);
}

fn emit_source(
    assembler: &mut X86_64Assembler,
    inputs: X86_64Register,
    bits: X86_64Register,
    argument_plan: &QuickDoubleArgumentProgram,
    forwarded_argument_mask: u8,
    scalar_registers: X86ScalarDoubleRegisterMap,
    instruction_set: X86DoubleInstructionSet,
    source: ScalarDoubleSource,
    scratch: X86_64FloatRegister,
) -> X86_64FloatRegister {
    match source {
        ScalarDoubleSource::Input(index) if forwarded_argument_mask & (1_u8 << index) != 0 => {
            let QuickDoubleSource::Temporary(index) = argument_plan.outputs[index as usize] else {
                unreachable!("forwarded Double argument must be a temporary")
            };
            argument_temporary(index as usize)
        }
        ScalarDoubleSource::Input(index) => {
            emit_load_f64(
                assembler,
                instruction_set,
                scratch,
                inputs,
                i32::from(index) * 8,
            );
            scratch
        }
        ScalarDoubleSource::Constant(value) => {
            assembler.move_immediate64(bits, value.to_bits() as i64);
            emit_gpr_bits_to_double(assembler, instruction_set, scratch, bits);
            scratch
        }
        ScalarDoubleSource::Temporary(index) => scalar_registers.temporary(index as usize),
        ScalarDoubleSource::Selection => X86_64FloatRegister::from_code(DOUBLE_SELECTION_REGISTER),
    }
}

#[inline]
fn argument_temporary(index: usize) -> X86_64FloatRegister {
    X86_64FloatRegister::from_code(FIRST_TEMPORARY + index as u8)
}

#[allow(clippy::too_many_arguments)]
fn emit_double_operation(
    assembler: &mut X86_64Assembler,
    instruction_set: X86DoubleInstructionSet,
    bits: X86_64Register,
    destination: X86_64FloatRegister,
    lhs: X86_64FloatRegister,
    rhs: X86_64FloatRegister,
    kind: ScalarDoubleOpKind,
    side_exits: &mut Vec<usize>,
) {
    if kind == ScalarDoubleOpKind::Divide {
        match instruction_set {
            X86DoubleInstructionSet::Sse2 => assembler.move_double_bits_to_gpr(bits, rhs),
            X86DoubleInstructionSet::Avx => assembler.move_double_bits_to_gpr_avx(bits, rhs),
        }
        assembler.shift_left_immediate8(bits, 1);
        assembler.compare_immediate8(bits, 0);
        side_exits.push(assembler.jump_equal_rel32());
    }

    match (instruction_set, kind) {
        (X86DoubleInstructionSet::Sse2, kind) => {
            assembler.move_double(destination, lhs);
            match kind {
                ScalarDoubleOpKind::Add => assembler.add_double(destination, rhs),
                ScalarDoubleOpKind::Subtract => assembler.subtract_double(destination, rhs),
                ScalarDoubleOpKind::Multiply => assembler.multiply_double(destination, rhs),
                ScalarDoubleOpKind::Divide => assembler.divide_double(destination, rhs),
            }
        }
        (X86DoubleInstructionSet::Avx, ScalarDoubleOpKind::Add) => {
            assembler.add_double_avx(destination, lhs, rhs)
        }
        (X86DoubleInstructionSet::Avx, ScalarDoubleOpKind::Subtract) => {
            assembler.subtract_double_avx(destination, lhs, rhs)
        }
        (X86DoubleInstructionSet::Avx, ScalarDoubleOpKind::Multiply) => {
            assembler.multiply_double_avx(destination, lhs, rhs)
        }
        (X86DoubleInstructionSet::Avx, ScalarDoubleOpKind::Divide) => {
            assembler.divide_double_avx(destination, lhs, rhs)
        }
    }
}

fn emit_move_double(
    assembler: &mut X86_64Assembler,
    instruction_set: X86DoubleInstructionSet,
    destination: X86_64FloatRegister,
    source: X86_64FloatRegister,
) {
    match instruction_set {
        X86DoubleInstructionSet::Sse2 => assembler.move_double(destination, source),
        X86DoubleInstructionSet::Avx => assembler.move_double_avx(destination, source),
    }
}

fn emit_load_f64(
    assembler: &mut X86_64Assembler,
    instruction_set: X86DoubleInstructionSet,
    destination: X86_64FloatRegister,
    base: X86_64Register,
    displacement: i32,
) {
    match instruction_set {
        X86DoubleInstructionSet::Sse2 => assembler.load_f64(destination, base, displacement),
        X86DoubleInstructionSet::Avx => assembler.load_f64_avx(destination, base, displacement),
    }
}

fn emit_store_f64(
    assembler: &mut X86_64Assembler,
    instruction_set: X86DoubleInstructionSet,
    base: X86_64Register,
    source: X86_64FloatRegister,
    displacement: i32,
) {
    match instruction_set {
        X86DoubleInstructionSet::Sse2 => assembler.store_f64(base, source, displacement),
        X86DoubleInstructionSet::Avx => assembler.store_f64_avx(base, source, displacement),
    }
}

fn emit_gpr_bits_to_double(
    assembler: &mut X86_64Assembler,
    instruction_set: X86DoubleInstructionSet,
    destination: X86_64FloatRegister,
    source: X86_64Register,
) {
    match instruction_set {
        X86DoubleInstructionSet::Sse2 => assembler.move_gpr_bits_to_double(destination, source),
        X86DoubleInstructionSet::Avx => assembler.move_gpr_bits_to_double_avx(destination, source),
    }
}

fn emit_publication(
    assembler: &mut X86_64Assembler,
    instruction_set: X86DoubleInstructionSet,
    state: X86_64Register,
    induction: X86_64Register,
    accumulator: X86_64FloatRegister,
    last_term: X86_64FloatRegister,
) {
    assembler.move_to_base_disp32(state, induction, 0);
    emit_store_f64(assembler, instruction_set, state, accumulator, 16);
    emit_store_f64(assembler, instruction_set, state, last_term, 24);
}
