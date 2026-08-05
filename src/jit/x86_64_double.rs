//! Exact-Double scalar leaf lowering for Linux x86-64.

use super::super::memory::ExecutableMemory;
use super::{X86_64Assembler, X86_64FloatRegister, X86_64Register, X86DoubleInstructionSet};
use crate::vm::function::{
    ScalarDoubleFunctionPlan, ScalarDoubleOp, ScalarDoubleOpKind, ScalarDoubleSource,
};
use std::cell::{Cell, OnceCell};
use std::fmt;
use std::io;
use std::mem::MaybeUninit;

const MAX_SCALAR_DOUBLE_INPUTS: usize = 8;
const MAX_SCALAR_DOUBLE_OPERATIONS: usize = 8;
const FIRST_DOUBLE_TEMPORARY_REGISTER: u8 = 2;
const NATIVE_DOUBLE_STATUS_SUCCESS: u32 = 0;
const NATIVE_DOUBLE_STATUS_SIDE_EXIT: u32 = 1;

/// Physical XMM assignment for target-neutral Double temporaries.
///
/// SSE2 arithmetic is destructive, so a result may reuse its LHS register
/// only when that temporary has no later IR use and is not the program output.
/// Every other result keeps its original one-register-per-operation slot. This
/// conservative fallback preserves the argument/leaf forwarding contract used
/// by the composed loop while removing moves from ordinary linear chains.
#[derive(Clone, Copy)]
pub(super) struct X86ScalarDoubleRegisterMap {
    temporaries: [X86_64FloatRegister; MAX_SCALAR_DOUBLE_OPERATIONS],
}

impl X86ScalarDoubleRegisterMap {
    pub(super) fn new(program: &crate::vm::function::ScalarDoubleProgram) -> Self {
        let mut last_use = [None; MAX_SCALAR_DOUBLE_OPERATIONS];
        for (operation_index, operation) in program.operations.iter().enumerate() {
            for source in [operation.lhs, operation.rhs] {
                if let ScalarDoubleSource::Temporary(index) = source {
                    last_use[index as usize] = Some(operation_index);
                }
            }
        }
        if let ScalarDoubleSource::Temporary(index) = program.output {
            last_use[index as usize] = Some(program.operations.len());
        }

        let mut temporaries = std::array::from_fn(|index| {
            X86_64FloatRegister::from_code(FIRST_DOUBLE_TEMPORARY_REGISTER + index as u8)
        });
        for (operation_index, operation) in program.operations.iter().enumerate() {
            let ScalarDoubleSource::Temporary(lhs) = operation.lhs else {
                continue;
            };
            if last_use[lhs as usize] == Some(operation_index) {
                temporaries[operation_index] = temporaries[lhs as usize];
            }
        }
        Self { temporaries }
    }

    #[inline(always)]
    pub(super) fn temporary(self, index: usize) -> X86_64FloatRegister {
        debug_assert!(index < MAX_SCALAR_DOUBLE_OPERATIONS);
        self.temporaries[index]
    }
}

pub const SCALAR_DOUBLE_JIT_HOT_THRESHOLD: u16 = 64;

#[derive(Debug)]
pub enum ScalarDoubleJitError {
    InvalidProgram(&'static str),
    Memory(io::Error),
    InputCount { expected: usize, actual: usize },
    InvalidNativeStatus(u32),
}

impl fmt::Display for ScalarDoubleJitError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::InvalidProgram(reason) => {
                write!(formatter, "invalid scalar Double program: {reason}")
            }
            Self::Memory(error) => write!(formatter, "cannot create executable memory: {error}"),
            Self::InputCount { expected, actual } => {
                write!(
                    formatter,
                    "Double JIT expected {expected} inputs but received {actual}"
                )
            }
            Self::InvalidNativeStatus(status) => {
                write!(formatter, "Double JIT returned an unknown status {status}")
            }
        }
    }
}

impl std::error::Error for ScalarDoubleJitError {
    fn source(&self) -> Option<&(dyn std::error::Error + 'static)> {
        match self {
            Self::Memory(error) => Some(error),
            _ => None,
        }
    }
}

impl From<io::Error> for ScalarDoubleJitError {
    fn from(error: io::Error) -> Self {
        Self::Memory(error)
    }
}

#[derive(Debug, Clone, Copy, PartialEq)]
pub enum ScalarDoubleJitOutcome {
    Value(f64),
    SideExit,
}

#[derive(Debug, Clone, Copy, PartialEq)]
pub enum ScalarDoubleJitDispatch {
    Interpret,
    Value(f64),
    SideExit,
}

pub struct ScalarDoubleJitCache {
    calls: Cell<u16>,
    compiled: OnceCell<Option<CompiledScalarDoubleProgram>>,
    native_entries: Cell<u64>,
    side_exits: Cell<u64>,
}

impl ScalarDoubleJitCache {
    pub const fn new() -> Self {
        Self {
            calls: Cell::new(0),
            compiled: OnceCell::new(),
            native_entries: Cell::new(0),
            side_exits: Cell::new(0),
        }
    }

    pub fn dispatch(
        &self,
        plan: &ScalarDoubleFunctionPlan,
        arguments: &[f64; MAX_SCALAR_DOUBLE_INPUTS],
    ) -> ScalarDoubleJitDispatch {
        if plan.program.operations.len() < 2 {
            return ScalarDoubleJitDispatch::Interpret;
        }
        if self.compiled.get().is_none() {
            let calls = self.calls.get().saturating_add(1);
            self.calls.set(calls);
            if calls < SCALAR_DOUBLE_JIT_HOT_THRESHOLD {
                return ScalarDoubleJitDispatch::Interpret;
            }
            let _ = self
                .compiled
                .set(CompiledScalarDoubleProgram::compile(plan).ok());
        }
        let Some(program) = self.compiled.get().and_then(Option::as_ref) else {
            return ScalarDoubleJitDispatch::Interpret;
        };
        self.native_entries
            .set(self.native_entries.get().saturating_add(1));
        match program.call(&arguments[..plan.public_args as usize]) {
            Ok(ScalarDoubleJitOutcome::Value(value)) => ScalarDoubleJitDispatch::Value(value),
            Ok(ScalarDoubleJitOutcome::SideExit) | Err(_) => {
                self.side_exits.set(self.side_exits.get().saturating_add(1));
                ScalarDoubleJitDispatch::SideExit
            }
        }
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

impl Default for ScalarDoubleJitCache {
    fn default() -> Self {
        Self::new()
    }
}

/// Native ABI: RDI points to exact Double inputs, RSI points to one
/// transactional output, and EAX returns success or side-exit status. XMM0
/// and XMM1 are scratch; XMM2-XMM9 form the resident temporary register bank.
/// Non-overlapping IR lifetimes may share one physical register.
pub struct CompiledScalarDoubleProgram {
    memory: ExecutableMemory,
    code: Box<[u8]>,
    input_count: usize,
}

impl CompiledScalarDoubleProgram {
    pub fn compile(plan: &ScalarDoubleFunctionPlan) -> Result<Self, ScalarDoubleJitError> {
        Self::compile_with_instruction_set(plan, X86DoubleInstructionSet::detected())
    }

    pub(super) fn compile_with_instruction_set(
        plan: &ScalarDoubleFunctionPlan,
        instruction_set: X86DoubleInstructionSet,
    ) -> Result<Self, ScalarDoubleJitError> {
        validate_scalar_double_plan(plan)?;
        let registers = X86ScalarDoubleRegisterMap::new(&plan.program);
        let mut assembler = X86_64Assembler::new();
        let mut side_exit_jumps = Vec::new();
        for (index, operation) in plan.program.operations.iter().copied().enumerate() {
            emit_scalar_double_operation(
                &mut assembler,
                instruction_set,
                registers,
                index,
                operation,
                &mut side_exit_jumps,
            );
        }
        let output = emit_scalar_double_source(
            &mut assembler,
            instruction_set,
            registers,
            plan.program.output,
            X86_64FloatRegister::from_code(0),
        );
        match instruction_set {
            X86DoubleInstructionSet::Sse2 => assembler.store_f64(X86_64Register::RSI, output, 0),
            X86DoubleInstructionSet::Avx => assembler.store_f64_avx(X86_64Register::RSI, output, 0),
        }
        if instruction_set == X86DoubleInstructionSet::Avx {
            assembler.vzeroupper();
        }
        assembler.move_immediate32_eax(NATIVE_DOUBLE_STATUS_SUCCESS);
        assembler.return_near();

        let side_exit = assembler.bytes.len();
        if instruction_set == X86DoubleInstructionSet::Avx {
            assembler.vzeroupper();
        }
        assembler.move_immediate32_eax(NATIVE_DOUBLE_STATUS_SIDE_EXIT);
        assembler.return_near();
        for jump in side_exit_jumps {
            assembler.patch_rel32(jump, side_exit);
        }

        let code = assembler.finish();
        let memory = ExecutableMemory::from_code(&code)?;
        Ok(Self {
            memory,
            code,
            input_count: plan.public_args as usize,
        })
    }

    pub fn call(&self, inputs: &[f64]) -> Result<ScalarDoubleJitOutcome, ScalarDoubleJitError> {
        if inputs.len() != self.input_count {
            return Err(ScalarDoubleJitError::InputCount {
                expected: self.input_count,
                actual: inputs.len(),
            });
        }
        type NativeFunction = unsafe extern "C" fn(*const f64, *mut f64) -> u32;
        let function: NativeFunction = unsafe { std::mem::transmute(self.memory.entry()) };
        let mut output = MaybeUninit::<f64>::uninit();
        let status = unsafe { function(inputs.as_ptr(), output.as_mut_ptr()) };
        match status {
            NATIVE_DOUBLE_STATUS_SUCCESS => Ok(ScalarDoubleJitOutcome::Value(unsafe {
                output.assume_init()
            })),
            NATIVE_DOUBLE_STATUS_SIDE_EXIT => Ok(ScalarDoubleJitOutcome::SideExit),
            status => Err(ScalarDoubleJitError::InvalidNativeStatus(status)),
        }
    }

    pub fn code(&self) -> &[u8] {
        &self.code
    }
}

fn validate_scalar_double_plan(
    plan: &ScalarDoubleFunctionPlan,
) -> Result<(), ScalarDoubleJitError> {
    if plan.public_args as usize > MAX_SCALAR_DOUBLE_INPUTS {
        return Err(ScalarDoubleJitError::InvalidProgram(
            "too many public inputs for the prototype ABI",
        ));
    }
    if plan.program.operations.len() > MAX_SCALAR_DOUBLE_OPERATIONS {
        return Err(ScalarDoubleJitError::InvalidProgram(
            "too many operations for the prototype register allocator",
        ));
    }
    for (index, operation) in plan.program.operations.iter().enumerate() {
        validate_scalar_double_source(operation.lhs, index, plan.public_args)?;
        validate_scalar_double_source(operation.rhs, index, plan.public_args)?;
    }
    validate_scalar_double_source(
        plan.program.output,
        plan.program.operations.len(),
        plan.public_args,
    )
}

fn validate_scalar_double_source(
    source: ScalarDoubleSource,
    available_temporaries: usize,
    input_count: u8,
) -> Result<(), ScalarDoubleJitError> {
    match source {
        ScalarDoubleSource::Input(index) if index >= u16::from(input_count) => Err(
            ScalarDoubleJitError::InvalidProgram("input index is outside the public ABI"),
        ),
        ScalarDoubleSource::Temporary(index) if index as usize >= available_temporaries => Err(
            ScalarDoubleJitError::InvalidProgram("temporary is used before it is defined"),
        ),
        _ => Ok(()),
    }
}

fn emit_scalar_double_operation(
    assembler: &mut X86_64Assembler,
    instruction_set: X86DoubleInstructionSet,
    registers: X86ScalarDoubleRegisterMap,
    index: usize,
    operation: ScalarDoubleOp,
    side_exit_jumps: &mut Vec<usize>,
) {
    let lhs = emit_scalar_double_source(
        assembler,
        instruction_set,
        registers,
        operation.lhs,
        X86_64FloatRegister::from_code(0),
    );
    let rhs = emit_scalar_double_source(
        assembler,
        instruction_set,
        registers,
        operation.rhs,
        X86_64FloatRegister::from_code(1),
    );
    let destination = registers.temporary(index);
    match (instruction_set, operation.kind) {
        (X86DoubleInstructionSet::Sse2, kind) => {
            assembler.move_double(destination, lhs);
            match kind {
                ScalarDoubleOpKind::Add => assembler.add_double(destination, rhs),
                ScalarDoubleOpKind::Subtract => assembler.subtract_double(destination, rhs),
                ScalarDoubleOpKind::Multiply => assembler.multiply_double(destination, rhs),
                ScalarDoubleOpKind::Divide => {
                    assembler.move_double_bits_to_gpr(X86_64Register::RAX, rhs);
                    assembler.shift_left_immediate8(X86_64Register::RAX, 1);
                    assembler.compare_immediate8(X86_64Register::RAX, 0);
                    side_exit_jumps.push(assembler.jump_equal_rel32());
                    assembler.divide_double(destination, rhs);
                }
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
            // Strip the sign bit without disturbing NaNs: only +0.0 and -0.0
            // become zero after this shift and therefore require PHP fallback.
            assembler.move_double_bits_to_gpr_avx(X86_64Register::RAX, rhs);
            assembler.shift_left_immediate8(X86_64Register::RAX, 1);
            assembler.compare_immediate8(X86_64Register::RAX, 0);
            side_exit_jumps.push(assembler.jump_equal_rel32());
            assembler.divide_double_avx(destination, lhs, rhs);
        }
    }
}

fn emit_scalar_double_source(
    assembler: &mut X86_64Assembler,
    instruction_set: X86DoubleInstructionSet,
    registers: X86ScalarDoubleRegisterMap,
    source: ScalarDoubleSource,
    scratch: X86_64FloatRegister,
) -> X86_64FloatRegister {
    match source {
        ScalarDoubleSource::Input(index) => {
            match instruction_set {
                X86DoubleInstructionSet::Sse2 => {
                    assembler.load_f64(scratch, X86_64Register::RDI, i32::from(index) * 8)
                }
                X86DoubleInstructionSet::Avx => {
                    assembler.load_f64_avx(scratch, X86_64Register::RDI, i32::from(index) * 8)
                }
            }
            scratch
        }
        ScalarDoubleSource::Constant(value) => {
            assembler.move_immediate64(X86_64Register::RAX, value.to_bits() as i64);
            match instruction_set {
                X86DoubleInstructionSet::Sse2 => {
                    assembler.move_gpr_bits_to_double(scratch, X86_64Register::RAX)
                }
                X86DoubleInstructionSet::Avx => {
                    assembler.move_gpr_bits_to_double_avx(scratch, X86_64Register::RAX)
                }
            }
            scratch
        }
        ScalarDoubleSource::Temporary(index) => registers.temporary(index as usize),
    }
}
