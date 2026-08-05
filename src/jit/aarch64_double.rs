//! Exact-Double scalar leaf lowering for macOS ARM64.

use super::super::memory::ExecutableMemory;
use super::{Arm64Assembler, Arm64Condition, Arm64FloatRegister, Arm64Register};
use crate::vm::function::{
    ScalarDoubleFunctionPlan, ScalarDoubleOp, ScalarDoubleOpKind, ScalarDoubleSource,
};
use std::cell::{Cell, OnceCell};
use std::fmt;
use std::io;
use std::mem::MaybeUninit;

const MAX_SCALAR_DOUBLE_INPUTS: usize = 8;
const MAX_SCALAR_DOUBLE_OPERATIONS: usize = 8;
const FIRST_DOUBLE_TEMPORARY_REGISTER: u8 = 16;
const NATIVE_DOUBLE_STATUS_SUCCESS: u32 = 0;
const NATIVE_DOUBLE_STATUS_SIDE_EXIT: u32 = 1;

pub const SCALAR_DOUBLE_JIT_HOT_THRESHOLD: u16 = 64;

#[derive(Debug)]
pub enum ScalarDoubleJitError {
    InvalidProgram(&'static str),
    BranchOutOfRange,
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
            Self::BranchOutOfRange => {
                formatter.write_str("ARM64 Double side-exit branch is out of range")
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

/// Native ABI: x0 points to exact Double inputs, x1 points to one transactional
/// output, and w0 returns success or side-exit status.
pub struct CompiledScalarDoubleProgram {
    memory: ExecutableMemory,
    code: Box<[u8]>,
    input_count: usize,
}

impl CompiledScalarDoubleProgram {
    pub fn compile(plan: &ScalarDoubleFunctionPlan) -> Result<Self, ScalarDoubleJitError> {
        validate_scalar_double_plan(plan)?;
        let mut assembler = Arm64Assembler::new();
        let mut side_exit_branches = Vec::new();
        for (index, operation) in plan.program.operations.iter().copied().enumerate() {
            emit_scalar_double_operation(&mut assembler, index, operation, &mut side_exit_branches);
        }
        let output = emit_scalar_double_source(
            &mut assembler,
            plan.program.output,
            Arm64FloatRegister::from_code(0),
        );
        assembler.store_f64(output, Arm64Register::X1, 0);
        assembler.move_immediate(Arm64Register::X0, i64::from(NATIVE_DOUBLE_STATUS_SUCCESS));
        assembler.ret();

        let side_exit_word = assembler.word_count();
        assembler.move_immediate(Arm64Register::X0, i64::from(NATIVE_DOUBLE_STATUS_SIDE_EXIT));
        assembler.ret();
        for branch in side_exit_branches {
            if !assembler.patch_conditional_branch(branch, side_exit_word) {
                return Err(ScalarDoubleJitError::BranchOutOfRange);
            }
        }

        let code = assembler.finish().into_boxed_slice();
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
    assembler: &mut Arm64Assembler,
    index: usize,
    operation: ScalarDoubleOp,
    side_exit_branches: &mut Vec<usize>,
) {
    let lhs = emit_scalar_double_source(assembler, operation.lhs, Arm64FloatRegister::from_code(0));
    let rhs = emit_scalar_double_source(assembler, operation.rhs, Arm64FloatRegister::from_code(1));
    let destination = scalar_double_temporary_register(index);
    match operation.kind {
        ScalarDoubleOpKind::Add => assembler.add_double(destination, lhs, rhs),
        ScalarDoubleOpKind::Subtract => assembler.subtract_double(destination, lhs, rhs),
        ScalarDoubleOpKind::Multiply => assembler.multiply_double(destination, lhs, rhs),
        ScalarDoubleOpKind::Divide => {
            assembler.compare_double_with_zero(rhs);
            side_exit_branches
                .push(assembler.conditional_branch_placeholder(Arm64Condition::Equal));
            assembler.divide_double(destination, lhs, rhs);
        }
    }
}

fn emit_scalar_double_source(
    assembler: &mut Arm64Assembler,
    source: ScalarDoubleSource,
    scratch: Arm64FloatRegister,
) -> Arm64FloatRegister {
    match source {
        ScalarDoubleSource::Input(index) => {
            assembler.load_f64(scratch, Arm64Register::X0, index * 8);
            scratch
        }
        ScalarDoubleSource::Constant(value) => {
            let bits = Arm64Register::from_code(2);
            assembler.move_immediate(bits, value.to_bits() as i64);
            assembler.move_register_bits_to_double(scratch, bits);
            scratch
        }
        ScalarDoubleSource::Temporary(index) => scalar_double_temporary_register(index as usize),
    }
}

#[inline]
fn scalar_double_temporary_register(index: usize) -> Arm64FloatRegister {
    debug_assert!(index < MAX_SCALAR_DOUBLE_OPERATIONS);
    Arm64FloatRegister::from_code(FIRST_DOUBLE_TEMPORARY_REGISTER + index as u8)
}
