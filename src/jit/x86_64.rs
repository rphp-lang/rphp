//! Minimal x86-64 SysV backend slice.
//!
//! Like the ARM64 backend, this encoder writes machine instructions directly;
//! it does not invoke an assembler, linker or external code-generation crate.

use super::memory::ExecutableMemory;
use super::straight::{
    NativeStraightLongLoopConfig, NativeStraightLongLoopOutcome, NativeStraightLongLoopResult,
    NativeStraightLongOperation, straight_long_remaining_range_proof,
};
use crate::vm::function::ScalarLongOpKind;
use crate::vm::quick::QuickLongOperand;
use std::fmt;
use std::io;

const X86_STRAIGHT_COMPLETED: u32 = 0;
const X86_STRAIGHT_CHUNK_EXHAUSTED: u32 = 1;
const X86_STRAIGHT_OPERATION_SIDE_EXIT: u32 = 6;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct X86_64Register(u8);

impl X86_64Register {
    pub const RAX: Self = Self(0);
    pub const RCX: Self = Self(1);
    pub const RDX: Self = Self(2);
    pub const RSI: Self = Self(6);
    pub const RDI: Self = Self(7);
    const R8: Self = Self(8);
    const R9: Self = Self(9);

    #[inline]
    const fn low_bits(self) -> u8 {
        self.0 & 7
    }

    #[inline]
    const fn extension(self) -> u8 {
        (self.0 >> 3) & 1
    }
}

#[derive(Debug, Default)]
pub struct X86_64Assembler {
    bytes: Vec<u8>,
}

impl X86_64Assembler {
    pub fn new() -> Self {
        Self::default()
    }

    /// Encode `MOV destination, source` using the register-direct r64/rm64 form.
    pub fn move_register(&mut self, destination: X86_64Register, source: X86_64Register) {
        self.emit_rex_w(destination, source);
        self.bytes.push(0x8b);
        self.emit_register_modrm(destination, source);
    }

    /// Encode `ADD destination, source` using the register-direct r64/rm64 form.
    pub fn add_register(&mut self, destination: X86_64Register, source: X86_64Register) {
        self.emit_rex_w(destination, source);
        self.bytes.push(0x03);
        self.emit_register_modrm(destination, source);
    }

    /// Encode the two-operand signed `IMUL destination, source` form.
    pub fn multiply_register(&mut self, destination: X86_64Register, source: X86_64Register) {
        self.emit_rex_w(destination, source);
        self.bytes.extend_from_slice(&[0x0f, 0xaf]);
        self.emit_register_modrm(destination, source);
    }

    fn move_from_base_disp32(
        &mut self,
        destination: X86_64Register,
        base: X86_64Register,
        displacement: i32,
    ) {
        debug_assert!(!matches!(base.low_bits(), 4 | 5));
        self.emit_rex_w(destination, base);
        self.bytes.push(0x8b);
        self.bytes
            .push(0x80 | (destination.low_bits() << 3) | base.low_bits());
        self.bytes.extend_from_slice(&displacement.to_le_bytes());
    }

    fn move_to_base_disp32(
        &mut self,
        base: X86_64Register,
        source: X86_64Register,
        displacement: i32,
    ) {
        debug_assert!(!matches!(base.low_bits(), 4 | 5));
        self.emit_rex_w(source, base);
        self.bytes.push(0x89);
        self.bytes
            .push(0x80 | (source.low_bits() << 3) | base.low_bits());
        self.bytes.extend_from_slice(&displacement.to_le_bytes());
    }

    fn move_immediate64(&mut self, destination: X86_64Register, immediate: i64) {
        self.bytes.push(0x48 | destination.extension());
        self.bytes.push(0xb8 + destination.low_bits());
        self.bytes.extend_from_slice(&immediate.to_le_bytes());
    }

    fn add_immediate8(&mut self, destination: X86_64Register, immediate: i8) {
        self.bytes.push(0x48 | destination.extension());
        self.bytes.push(0x83);
        self.bytes.push(0xc0 | destination.low_bits());
        self.bytes.push(immediate as u8);
    }

    fn subtract_immediate8(&mut self, destination: X86_64Register, immediate: i8) {
        self.bytes.push(0x48 | destination.extension());
        self.bytes.push(0x83);
        self.bytes.push(0xe8 | destination.low_bits());
        self.bytes.push(immediate as u8);
    }

    fn compare_register(&mut self, lhs: X86_64Register, rhs: X86_64Register) {
        self.emit_rex_w(lhs, rhs);
        self.bytes.push(0x3b);
        self.emit_register_modrm(lhs, rhs);
    }

    fn jump_greater_or_equal_rel32(&mut self) -> usize {
        self.emit_conditional_jump_rel32(0x8d)
    }

    fn jump_less_than_rel32(&mut self) -> usize {
        self.emit_conditional_jump_rel32(0x8c)
    }

    fn jump_not_equal_rel32(&mut self) -> usize {
        self.emit_conditional_jump_rel32(0x85)
    }

    fn jump_overflow_rel32(&mut self) -> usize {
        self.emit_conditional_jump_rel32(0x80)
    }

    fn emit_conditional_jump_rel32(&mut self, opcode: u8) -> usize {
        self.bytes.extend_from_slice(&[0x0f, opcode]);
        let displacement = self.bytes.len();
        self.bytes.extend_from_slice(&0i32.to_le_bytes());
        displacement
    }

    fn patch_rel32(&mut self, displacement: usize, target: usize) {
        let next_instruction = displacement + std::mem::size_of::<i32>();
        let relative = i64::try_from(target).unwrap() - i64::try_from(next_instruction).unwrap();
        let relative = i32::try_from(relative).expect("x86 prototype branch exceeds rel32 range");
        self.bytes[displacement..next_instruction].copy_from_slice(&relative.to_le_bytes());
    }

    fn clear_eax(&mut self) {
        self.bytes.extend_from_slice(&[0x31, 0xc0]);
    }

    fn move_immediate32_eax(&mut self, immediate: u32) {
        self.bytes.push(0xb8);
        self.bytes.extend_from_slice(&immediate.to_le_bytes());
    }

    pub fn return_near(&mut self) {
        self.bytes.push(0xc3);
    }

    pub fn finish(self) -> Box<[u8]> {
        self.bytes.into_boxed_slice()
    }

    fn emit_rex_w(&mut self, reg: X86_64Register, rm: X86_64Register) {
        self.bytes
            .push(0x48 | (reg.extension() << 2) | rm.extension());
    }

    fn emit_register_modrm(&mut self, reg: X86_64Register, rm: X86_64Register) {
        self.bytes
            .push(0xc0 | (reg.low_bits() << 3) | rm.low_bits());
    }
}

#[derive(Debug)]
pub enum X86StraightLongLoopError {
    UnsupportedConfig(&'static str),
    ZeroIterationBudget,
    InvalidStatus(u32),
    Memory(io::Error),
}

impl fmt::Display for X86StraightLongLoopError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::UnsupportedConfig(message) => formatter.write_str(message),
            Self::ZeroIterationBudget => {
                formatter.write_str("x86 JIT iteration budget must be non-zero")
            }
            Self::InvalidStatus(status) => write!(formatter, "invalid x86 JIT status {status}"),
            Self::Memory(error) => write!(formatter, "executable memory error: {error}"),
        }
    }
}

impl std::error::Error for X86StraightLongLoopError {}

impl From<io::Error> for X86StraightLongLoopError {
    fn from(error: io::Error) -> Self {
        Self::Memory(error)
    }
}

/// First straight-IR lowering on x86-64. It accepts one additive recurrence
/// and emits both an unchecked range-proven entry and a checked side-exit entry.
pub struct CompiledX86StraightLongLoop {
    memory: ExecutableMemory,
    code: Box<[u8]>,
    config: NativeStraightLongLoopConfig,
    bound: i64,
    checked_entry_offset: usize,
    chunk_entry_offset: usize,
    checked_chunk_entry_offset: usize,
}

fn emit_additive_recurrence_loop(
    induction: u16,
    accumulator: u16,
    result: u16,
    destination: u16,
    bound: i64,
    checked: bool,
    budgeted: bool,
) -> Box<[u8]> {
    let mut assembler = X86_64Assembler::new();
    let slots = X86_64Register::RDI;
    let induction_register = X86_64Register::RAX;
    let bound_register = X86_64Register::RCX;
    let accumulator_register = X86_64Register::RDX;
    let candidate_register = X86_64Register::R8;
    let previous_result_register = X86_64Register::R9;
    let remaining_register = X86_64Register::RSI;
    let displacement = |slot: u16| i32::from(slot) * 8;

    assembler.move_from_base_disp32(induction_register, slots, displacement(induction));
    assembler.move_from_base_disp32(accumulator_register, slots, displacement(accumulator));
    if checked && result != destination {
        assembler.move_from_base_disp32(previous_result_register, slots, displacement(result));
    }
    assembler.move_immediate64(bound_register, bound);
    assembler.compare_register(induction_register, bound_register);
    let completed_jump = assembler.jump_greater_or_equal_rel32();
    let loop_start = assembler.bytes.len();
    let overflow_jump = if checked {
        assembler.move_register(candidate_register, accumulator_register);
        assembler.add_register(candidate_register, induction_register);
        let overflow_jump = assembler.jump_overflow_rel32();
        assembler.move_register(accumulator_register, candidate_register);
        if result != destination {
            assembler.move_register(previous_result_register, accumulator_register);
        }
        Some(overflow_jump)
    } else {
        assembler.add_register(accumulator_register, induction_register);
        None
    };
    assembler.add_immediate8(induction_register, 1);
    assembler.compare_register(induction_register, bound_register);
    let completed_after_iteration_jump = budgeted.then(|| assembler.jump_greater_or_equal_rel32());
    let loop_jump = if budgeted {
        assembler.subtract_immediate8(remaining_register, 1);
        assembler.jump_not_equal_rel32()
    } else {
        assembler.jump_less_than_rel32()
    };

    if budgeted {
        assembler.move_to_base_disp32(slots, induction_register, displacement(induction));
        assembler.move_to_base_disp32(slots, accumulator_register, displacement(destination));
        if result != destination {
            assembler.move_to_base_disp32(slots, accumulator_register, displacement(result));
        }
        assembler.move_immediate32_eax(X86_STRAIGHT_CHUNK_EXHAUSTED);
        assembler.return_near();
    }

    let completed = assembler.bytes.len();
    assembler.patch_rel32(completed_jump, completed);
    if let Some(completed_after_iteration_jump) = completed_after_iteration_jump {
        assembler.patch_rel32(completed_after_iteration_jump, completed);
    }
    assembler.patch_rel32(loop_jump, loop_start);
    assembler.move_to_base_disp32(slots, induction_register, displacement(induction));
    assembler.move_to_base_disp32(slots, accumulator_register, displacement(destination));
    if result != destination {
        assembler.move_to_base_disp32(slots, accumulator_register, displacement(result));
    }
    assembler.clear_eax();
    assembler.return_near();

    if let Some(overflow_jump) = overflow_jump {
        let side_exit = assembler.bytes.len();
        assembler.patch_rel32(overflow_jump, side_exit);
        assembler.move_to_base_disp32(slots, induction_register, displacement(induction));
        assembler.move_to_base_disp32(slots, accumulator_register, displacement(destination));
        if result != destination {
            assembler.move_to_base_disp32(slots, previous_result_register, displacement(result));
        }
        assembler.move_immediate32_eax(X86_STRAIGHT_OPERATION_SIDE_EXIT);
        assembler.return_near();
    }

    assembler.finish()
}

impl CompiledX86StraightLongLoop {
    pub fn compile(config: NativeStraightLongLoopConfig) -> Result<Self, X86StraightLongLoopError> {
        if config.operation_count != 1 {
            return Err(X86StraightLongLoopError::UnsupportedConfig(
                "x86 straight-loop prototype requires exactly one operation",
            ));
        }
        let QuickLongOperand::Const(bound) = config.bound else {
            return Err(X86StraightLongLoopError::UnsupportedConfig(
                "x86 straight-loop prototype requires a constant bound",
            ));
        };
        let NativeStraightLongOperation::BinaryAssign {
            kind: ScalarLongOpKind::Add,
            lhs: QuickLongOperand::Slot(lhs),
            rhs: QuickLongOperand::Slot(rhs),
            result,
            destination,
        } = config.operations[0]
        else {
            return Err(X86StraightLongLoopError::UnsupportedConfig(
                "x86 straight-loop prototype requires BinaryAssign(Add)",
            ));
        };
        let induction = config.induction_slot;
        let accumulator = if lhs == destination && rhs == induction {
            lhs
        } else if rhs == destination && lhs == induction {
            rhs
        } else {
            return Err(X86StraightLongLoopError::UnsupportedConfig(
                "x86 straight-loop prototype requires destination plus induction",
            ));
        };
        for slot in [induction, accumulator, result, destination] {
            if slot >= 64 {
                return Err(X86StraightLongLoopError::UnsupportedConfig(
                    "x86 straight-loop slot exceeds the fixed shadow",
                ));
            }
        }

        let fast_code = emit_additive_recurrence_loop(
            induction,
            accumulator,
            result,
            destination,
            bound,
            false,
            false,
        );
        let checked_entry_offset = fast_code.len();
        let checked_code = emit_additive_recurrence_loop(
            induction,
            accumulator,
            result,
            destination,
            bound,
            true,
            false,
        );
        let mut code = fast_code.into_vec();
        code.extend_from_slice(&checked_code);
        let chunk_entry_offset = code.len();
        let chunk_code = emit_additive_recurrence_loop(
            induction,
            accumulator,
            result,
            destination,
            bound,
            false,
            true,
        );
        code.extend_from_slice(&chunk_code);
        let checked_chunk_entry_offset = code.len();
        let checked_chunk_code = emit_additive_recurrence_loop(
            induction,
            accumulator,
            result,
            destination,
            bound,
            true,
            true,
        );
        code.extend_from_slice(&checked_chunk_code);
        let code = code.into_boxed_slice();
        let memory = ExecutableMemory::from_code(&code)?;
        Ok(Self {
            memory,
            code,
            config,
            bound,
            checked_entry_offset,
            chunk_entry_offset,
            checked_chunk_entry_offset,
        })
    }

    pub fn call(
        &self,
        slots: &mut [i64; 64],
    ) -> Result<NativeStraightLongLoopResult, X86StraightLongLoopError> {
        if slots[self.config.induction_slot as usize] >= self.bound {
            return Ok(NativeStraightLongLoopResult {
                outcome: NativeStraightLongLoopOutcome::Completed,
                failed_operation: None,
            });
        }
        let entry = if straight_long_remaining_range_proof(&self.config, slots).is_some() {
            self.memory.entry()
        } else {
            unsafe { self.memory.entry().add(self.checked_entry_offset) }
        };
        type NativeFunction = unsafe extern "C" fn(*mut i64) -> u32;
        let function: NativeFunction = unsafe { std::mem::transmute(entry) };
        let status = unsafe { function(slots.as_mut_ptr()) };
        self.decode_status(status)
    }

    /// Execute no more than `iteration_budget` loop iterations. This is the
    /// VM-facing safepoint boundary; exact scalar state is published on every
    /// chunk return, completion, or checked operation side exit.
    pub fn call_chunk(
        &self,
        slots: &mut [i64; 64],
        iteration_budget: u64,
    ) -> Result<NativeStraightLongLoopResult, X86StraightLongLoopError> {
        if iteration_budget == 0 {
            return Err(X86StraightLongLoopError::ZeroIterationBudget);
        }
        if slots[self.config.induction_slot as usize] >= self.bound {
            return Ok(NativeStraightLongLoopResult {
                outcome: NativeStraightLongLoopOutcome::Completed,
                failed_operation: None,
            });
        }
        let entry_offset = if straight_long_remaining_range_proof(&self.config, slots).is_some() {
            self.chunk_entry_offset
        } else {
            self.checked_chunk_entry_offset
        };
        let entry = unsafe { self.memory.entry().add(entry_offset) };
        type NativeFunction = unsafe extern "C" fn(*mut i64, u64) -> u32;
        let function: NativeFunction = unsafe { std::mem::transmute(entry) };
        let status = unsafe { function(slots.as_mut_ptr(), iteration_budget) };
        self.decode_status(status)
    }

    fn decode_status(
        &self,
        status: u32,
    ) -> Result<NativeStraightLongLoopResult, X86StraightLongLoopError> {
        match status {
            X86_STRAIGHT_COMPLETED => Ok(NativeStraightLongLoopResult {
                outcome: NativeStraightLongLoopOutcome::Completed,
                failed_operation: None,
            }),
            X86_STRAIGHT_CHUNK_EXHAUSTED => Ok(NativeStraightLongLoopResult {
                outcome: NativeStraightLongLoopOutcome::ChunkExhausted,
                failed_operation: None,
            }),
            X86_STRAIGHT_OPERATION_SIDE_EXIT => Ok(NativeStraightLongLoopResult {
                outcome: NativeStraightLongLoopOutcome::OperationSideExit,
                failed_operation: Some(0),
            }),
            status => Err(X86StraightLongLoopError::InvalidStatus(status)),
        }
    }

    pub fn code(&self) -> &[u8] {
        &self.code
    }
}

/// First x86-64 ABI vertical slice: `(first + second) * multiplier`.
pub struct CompiledX86AddMultiply {
    memory: ExecutableMemory,
    code: Box<[u8]>,
}

impl CompiledX86AddMultiply {
    pub fn compile() -> io::Result<Self> {
        let mut assembler = X86_64Assembler::new();
        // System V AMD64 passes the first three integer arguments in RDI, RSI,
        // and RDX. Integer return values use RAX.
        assembler.move_register(X86_64Register::RAX, X86_64Register::RDI);
        assembler.add_register(X86_64Register::RAX, X86_64Register::RSI);
        assembler.multiply_register(X86_64Register::RAX, X86_64Register::RDX);
        assembler.return_near();
        let code = assembler.finish();
        let memory = ExecutableMemory::from_code(&code)?;
        Ok(Self { memory, code })
    }

    pub fn call(&self, first: i64, second: i64, multiplier: i64) -> i64 {
        type NativeFunction = unsafe extern "C" fn(i64, i64, i64) -> i64;
        let function: NativeFunction = unsafe { std::mem::transmute(self.memory.entry()) };
        unsafe { function(first, second, multiplier) }
    }

    pub fn code(&self) -> &[u8] {
        &self.code
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::jit::NATIVE_STRAIGHT_LONG_MAX_OPERATIONS;

    fn additive_recurrence(bound: i64, reversed: bool) -> NativeStraightLongLoopConfig {
        let mut operations =
            [NativeStraightLongOperation::Unused; NATIVE_STRAIGHT_LONG_MAX_OPERATIONS];
        let (lhs, rhs) = if reversed {
            (QuickLongOperand::Slot(0), QuickLongOperand::Slot(1))
        } else {
            (QuickLongOperand::Slot(1), QuickLongOperand::Slot(0))
        };
        operations[0] = NativeStraightLongOperation::BinaryAssign {
            kind: ScalarLongOpKind::Add,
            lhs,
            rhs,
            result: 2,
            destination: 1,
        };
        NativeStraightLongLoopConfig {
            induction_slot: 0,
            bound: QuickLongOperand::Const(bound),
            operations,
            operation_count: 1,
            post_result: None,
        }
    }

    #[test]
    fn encoder_produces_exact_sysv_add_multiply_bytes() {
        let program = CompiledX86AddMultiply::compile().unwrap();
        assert_eq!(
            program.code(),
            [
                0x48, 0x8b, 0xc7, // MOV RAX, RDI
                0x48, 0x03, 0xc6, // ADD RAX, RSI
                0x48, 0x0f, 0xaf, 0xc2, // IMUL RAX, RDX
                0xc3, // RET
            ]
        );
    }

    #[test]
    fn generated_code_executes_through_the_sysv_abi() {
        let program = CompiledX86AddMultiply::compile().unwrap();
        assert_eq!(program.call(12, -5, 9), 63);
        assert_eq!(program.call(-8, 3, -4), 20);
    }

    #[test]
    fn encoder_sets_rex_extensions_for_high_registers() {
        let mut assembler = X86_64Assembler::new();
        assembler.move_register(X86_64Register::R8, X86_64Register::R9);
        assert_eq!(&*assembler.finish(), &[0x4d, 0x8b, 0xc1]);
    }

    #[test]
    fn range_proven_loop_executes_and_publishes_exact_slots() {
        let program =
            CompiledX86StraightLongLoop::compile(additive_recurrence(100, false)).unwrap();
        let mut slots = [0_i64; 64];
        slots[1] = 5;
        let result = program.call(&mut slots).unwrap();
        assert_eq!(result.outcome, NativeStraightLongLoopOutcome::Completed);
        assert_eq!(result.failed_operation, None);
        assert_eq!(slots[0], 100);
        assert_eq!(slots[1], 4_955);
        assert_eq!(slots[2], 4_955);

        assert!(
            program
                .code()
                .windows(6)
                .any(|bytes| bytes == [0x0f, 0x8d, 0x10, 0, 0, 0])
        );
        assert!(
            program
                .code()
                .windows(6)
                .any(|bytes| bytes == [0x0f, 0x8c, 0xf0, 0xff, 0xff, 0xff])
        );
        assert!(
            program
                .code()
                .windows(7)
                .any(|bytes| bytes == [0x48, 0x89, 0x97, 0x10, 0, 0, 0])
        );
    }

    #[test]
    fn chunk_entry_publishes_exact_safepoint_and_resumes_to_completion() {
        let program = CompiledX86StraightLongLoop::compile(additive_recurrence(10, false)).unwrap();
        let mut slots = [0_i64; 64];
        slots[1] = 5;

        let first = program.call_chunk(&mut slots, 3).unwrap();
        assert_eq!(
            first,
            NativeStraightLongLoopResult {
                outcome: NativeStraightLongLoopOutcome::ChunkExhausted,
                failed_operation: None,
            }
        );
        assert_eq!(&slots[..3], &[3, 8, 8]);

        let second = program.call_chunk(&mut slots, 7).unwrap();
        assert_eq!(
            second,
            NativeStraightLongLoopResult {
                outcome: NativeStraightLongLoopOutcome::Completed,
                failed_operation: None,
            }
        );
        assert_eq!(&slots[..3], &[10, 50, 50]);

        let mut exact = [0_i64; 64];
        exact[1] = 5;
        let exact_result = program.call_chunk(&mut exact, 10).unwrap();
        assert_eq!(
            exact_result.outcome,
            NativeStraightLongLoopOutcome::Completed
        );
        assert_eq!(&exact[..3], &[10, 50, 50]);
    }

    #[test]
    fn chunk_entry_rejects_zero_budget_and_retains_checked_side_exit() {
        let program = CompiledX86StraightLongLoop::compile(additive_recurrence(2, false)).unwrap();
        let mut slots = [0_i64; 64];
        assert!(matches!(
            program.call_chunk(&mut slots, 0),
            Err(X86StraightLongLoopError::ZeroIterationBudget)
        ));

        slots[0] = 1;
        slots[1] = i64::MAX;
        slots[2] = 77;
        let side_exit = program.call_chunk(&mut slots, 1).unwrap();
        assert_eq!(
            side_exit,
            NativeStraightLongLoopResult {
                outcome: NativeStraightLongLoopOutcome::OperationSideExit,
                failed_operation: Some(0),
            }
        );
        assert_eq!(&slots[..3], &[1, i64::MAX, 77]);
    }

    #[test]
    fn checked_side_exit_preserves_state_before_first_failed_operation() {
        let program = CompiledX86StraightLongLoop::compile(additive_recurrence(2, false)).unwrap();
        let mut slots = [0_i64; 64];
        slots[0] = 1;
        slots[1] = i64::MAX;
        slots[2] = 77;
        let result = program.call(&mut slots).unwrap();
        assert_eq!(
            result.outcome,
            NativeStraightLongLoopOutcome::OperationSideExit
        );
        assert_eq!(result.failed_operation, Some(0));
        assert_eq!(&slots[..3], &[1, i64::MAX, 77]);
        assert!(
            !program.code()[..program.checked_entry_offset]
                .windows(2)
                .any(|bytes| bytes == [0x0f, 0x80])
        );
        assert!(
            program.code()[program.checked_entry_offset..]
                .windows(2)
                .any(|bytes| bytes == [0x0f, 0x80])
        );
    }

    #[test]
    fn checked_side_exit_publishes_last_successful_iteration() {
        let program = CompiledX86StraightLongLoop::compile(additive_recurrence(2, false)).unwrap();
        let mut slots = [0_i64; 64];
        slots[1] = i64::MAX;
        slots[2] = 77;
        let result = program.call(&mut slots).unwrap();
        assert_eq!(
            result.outcome,
            NativeStraightLongLoopOutcome::OperationSideExit
        );
        assert_eq!(result.failed_operation, Some(0));
        assert_eq!(&slots[..3], &[1, i64::MAX, i64::MAX]);
    }

    #[test]
    fn reversed_addition_and_empty_range_preserve_semantics() {
        let program = CompiledX86StraightLongLoop::compile(additive_recurrence(4, true)).unwrap();
        let mut slots = [0_i64; 64];
        slots[0] = -2;
        slots[1] = 10;
        program.call(&mut slots).unwrap();
        assert_eq!(&slots[..3], &[4, 13, 13]);

        let mut empty = [0_i64; 64];
        empty[0] = 4;
        empty[1] = 9;
        empty[2] = 81;
        program.call(&mut empty).unwrap();
        assert_eq!(&empty[..3], &[4, 9, 81]);
    }
}
