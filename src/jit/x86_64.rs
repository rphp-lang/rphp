//! Minimal x86-64 SysV backend slice.
//!
//! Like the ARM64 backend, this encoder writes machine instructions directly;
//! it does not invoke an assembler, linker or external code-generation crate.

use super::memory::ExecutableMemory;
use super::straight::{
    NativeStraightLongConditionOperand, NativeStraightLongLoopConfig,
    NativeStraightLongLoopOutcome, NativeStraightLongLoopResult, NativeStraightLongOperation,
    straight_long_remaining_range_proof,
};
use crate::vm::function::{ScalarLongConditionKind, ScalarLongOpKind};
use crate::vm::quick::QuickLongOperand;
use std::cell::{Cell, OnceCell};
use std::fmt;
use std::io;

const X86_STRAIGHT_COMPLETED: u32 = 0;
const X86_STRAIGHT_CHUNK_EXHAUSTED: u32 = 1;
const X86_STRAIGHT_OPERATION_SIDE_EXIT: u32 = 6;
const X86_STRAIGHT_SAFEPOINT_INTERVAL: u16 = 1024;

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
    const R10: Self = Self(10);
    const R11: Self = Self(11);

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

    /// Encode `SUB destination, source` using the register-direct r64/rm64 form.
    pub fn subtract_register(&mut self, destination: X86_64Register, source: X86_64Register) {
        self.emit_rex_w(destination, source);
        self.bytes.push(0x2b);
        self.emit_register_modrm(destination, source);
    }

    /// Encode `AND destination, source` using the register-direct r64/rm64 form.
    pub fn and_register(&mut self, destination: X86_64Register, source: X86_64Register) {
        self.emit_rex_w(destination, source);
        self.bytes.push(0x23);
        self.emit_register_modrm(destination, source);
    }

    /// Encode `XOR destination, source` using the register-direct r64/rm64 form.
    pub fn xor_register(&mut self, destination: X86_64Register, source: X86_64Register) {
        self.emit_rex_w(destination, source);
        self.bytes.push(0x33);
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

    fn arithmetic_shift_right_immediate8(&mut self, destination: X86_64Register, immediate: u8) {
        self.bytes.push(0x48 | destination.extension());
        self.bytes.push(0xc1);
        self.bytes.push(0xf8 | destination.low_bits());
        self.bytes.push(immediate);
    }

    fn compare_register(&mut self, lhs: X86_64Register, rhs: X86_64Register) {
        self.emit_rex_w(lhs, rhs);
        self.bytes.push(0x3b);
        self.emit_register_modrm(lhs, rhs);
    }

    fn compare_immediate8(&mut self, register: X86_64Register, immediate: i8) {
        self.bytes.push(0x48 | register.extension());
        self.bytes.push(0x83);
        self.bytes.push(0xf8 | register.low_bits());
        self.bytes.push(immediate as u8);
    }

    fn sign_extend_rax_into_rdx(&mut self) {
        self.bytes.extend_from_slice(&[0x48, 0x99]);
    }

    fn signed_divide(&mut self, divisor: X86_64Register) {
        self.bytes.push(0x48 | divisor.extension());
        self.bytes.push(0xf7);
        self.bytes.push(0xf8 | divisor.low_bits());
    }

    fn compare_byte_base_immediate8(&mut self, base: X86_64Register, immediate: u8) {
        debug_assert!(!matches!(base.low_bits(), 4 | 5));
        if base.extension() != 0 {
            self.bytes.push(0x41);
        }
        self.bytes.push(0x80);
        self.bytes.push((7 << 3) | base.low_bits());
        self.bytes.push(immediate);
    }

    fn jump_greater_or_equal_rel32(&mut self) -> usize {
        self.emit_conditional_jump_rel32(0x8d)
    }

    fn jump_greater_than_rel32(&mut self) -> usize {
        self.emit_conditional_jump_rel32(0x8f)
    }

    fn jump_less_than_rel32(&mut self) -> usize {
        self.emit_conditional_jump_rel32(0x8c)
    }

    fn jump_less_or_equal_rel32(&mut self) -> usize {
        self.emit_conditional_jump_rel32(0x8e)
    }

    fn jump_equal_rel32(&mut self) -> usize {
        self.emit_conditional_jump_rel32(0x84)
    }

    fn jump_not_equal_rel32(&mut self) -> usize {
        self.emit_conditional_jump_rel32(0x85)
    }

    fn jump_overflow_rel32(&mut self) -> usize {
        self.emit_conditional_jump_rel32(0x80)
    }

    fn jump_rel32(&mut self) -> usize {
        self.bytes.push(0xe9);
        let displacement = self.bytes.len();
        self.bytes.extend_from_slice(&0i32.to_le_bytes());
        displacement
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

/// Shared straight-IR lowering on x86-64. It retains a register-specialized
/// additive recurrence and otherwise emits structured scalar programs with
/// unchecked range-proven and checked exact-side-exit entries.
pub struct CompiledX86StraightLongLoop {
    memory: ExecutableMemory,
    code: Box<[u8]>,
    config: NativeStraightLongLoopConfig,
    checked_entry_offset: usize,
    chunk_entry_offset: usize,
    checked_chunk_entry_offset: usize,
    polling_entry_offset: usize,
}

fn emit_additive_recurrence_loop(
    induction: u16,
    accumulator: u16,
    result: u16,
    destination: u16,
    bound: QuickLongOperand,
    checked: bool,
    budgeted: bool,
    polling_interval: Option<u16>,
) -> Box<[u8]> {
    debug_assert!(!(budgeted && polling_interval.is_some()));
    let mut assembler = X86_64Assembler::new();
    let slots = X86_64Register::RDI;
    let induction_register = X86_64Register::RAX;
    let bound_register = X86_64Register::RCX;
    let accumulator_register = X86_64Register::RDX;
    let candidate_register = X86_64Register::R8;
    let previous_result_register = X86_64Register::R9;
    let remaining_register = X86_64Register::RSI;
    let polling_remaining_register = X86_64Register::R8;
    let interrupt_pointer_register = X86_64Register::RSI;
    let displacement = |slot: u16| i32::from(slot) * 8;

    assembler.move_from_base_disp32(induction_register, slots, displacement(induction));
    assembler.move_from_base_disp32(accumulator_register, slots, displacement(accumulator));
    if checked && result != destination {
        assembler.move_from_base_disp32(previous_result_register, slots, displacement(result));
    }
    match bound {
        QuickLongOperand::Slot(slot) => {
            assembler.move_from_base_disp32(bound_register, slots, displacement(slot));
        }
        QuickLongOperand::Const(bound) => assembler.move_immediate64(bound_register, bound),
    }
    if let Some(interval) = polling_interval {
        assembler.move_immediate64(polling_remaining_register, i64::from(interval));
    }
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
    let completed_after_iteration_jump =
        (budgeted || polling_interval.is_some()).then(|| assembler.jump_greater_or_equal_rel32());
    let mut loop_jumps = Vec::with_capacity(2);
    let interrupt_jump = if budgeted {
        assembler.subtract_immediate8(remaining_register, 1);
        loop_jumps.push(assembler.jump_not_equal_rel32());
        None
    } else if let Some(interval) = polling_interval {
        assembler.subtract_immediate8(polling_remaining_register, 1);
        loop_jumps.push(assembler.jump_not_equal_rel32());
        assembler.compare_byte_base_immediate8(interrupt_pointer_register, 0);
        let interrupt_jump = assembler.jump_not_equal_rel32();
        assembler.move_immediate64(polling_remaining_register, i64::from(interval));
        loop_jumps.push(assembler.jump_rel32());
        Some(interrupt_jump)
    } else {
        loop_jumps.push(assembler.jump_less_than_rel32());
        None
    };

    if budgeted || polling_interval.is_some() {
        let chunk_exhausted = assembler.bytes.len();
        if let Some(interrupt_jump) = interrupt_jump {
            assembler.patch_rel32(interrupt_jump, chunk_exhausted);
        }
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
    for loop_jump in loop_jumps {
        assembler.patch_rel32(loop_jump, loop_start);
    }
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

fn emit_linear_operand(
    assembler: &mut X86_64Assembler,
    operand: QuickLongOperand,
    destination: X86_64Register,
    induction_slot: u16,
    induction_register: X86_64Register,
) {
    match operand {
        QuickLongOperand::Slot(slot) if slot == induction_slot => {
            assembler.move_register(destination, induction_register)
        }
        QuickLongOperand::Slot(slot) => {
            assembler.move_from_base_disp32(destination, X86_64Register::RDI, i32::from(slot) * 8)
        }
        QuickLongOperand::Const(value) => assembler.move_immediate64(destination, value),
    }
}

fn emit_linear_condition_operand(
    assembler: &mut X86_64Assembler,
    operand: NativeStraightLongConditionOperand,
    destination: X86_64Register,
    scratch: X86_64Register,
    induction_slot: u16,
    induction_register: X86_64Register,
) {
    match operand {
        NativeStraightLongConditionOperand::Source(source) => {
            emit_linear_operand(
                assembler,
                source,
                destination,
                induction_slot,
                induction_register,
            );
        }
        NativeStraightLongConditionOperand::BitwiseAnd { lhs, rhs } => {
            emit_linear_operand(
                assembler,
                lhs,
                destination,
                induction_slot,
                induction_register,
            );
            emit_linear_operand(assembler, rhs, scratch, induction_slot, induction_register);
            assembler.and_register(destination, scratch);
        }
    }
}

fn emit_false_condition_jump(
    assembler: &mut X86_64Assembler,
    kind: ScalarLongConditionKind,
) -> usize {
    match kind {
        ScalarLongConditionKind::Equal => assembler.jump_not_equal_rel32(),
        ScalarLongConditionKind::NotEqual => assembler.jump_equal_rel32(),
        ScalarLongConditionKind::LessThan => assembler.jump_greater_or_equal_rel32(),
        ScalarLongConditionKind::LessThanOrEqual => assembler.jump_greater_than_rel32(),
    }
}

fn emit_guard_mismatch_jump(
    assembler: &mut X86_64Assembler,
    kind: ScalarLongConditionKind,
    expected: bool,
) -> usize {
    match (kind, expected) {
        (ScalarLongConditionKind::Equal, true) | (ScalarLongConditionKind::NotEqual, false) => {
            assembler.jump_not_equal_rel32()
        }
        (ScalarLongConditionKind::Equal, false) | (ScalarLongConditionKind::NotEqual, true) => {
            assembler.jump_equal_rel32()
        }
        (ScalarLongConditionKind::LessThan, true) => assembler.jump_greater_or_equal_rel32(),
        (ScalarLongConditionKind::LessThan, false) => assembler.jump_less_than_rel32(),
        (ScalarLongConditionKind::LessThanOrEqual, true) => assembler.jump_greater_than_rel32(),
        (ScalarLongConditionKind::LessThanOrEqual, false) => assembler.jump_less_or_equal_rel32(),
    }
}

fn signed_power_of_two_remainder_mask(divisor: i64) -> Option<i64> {
    let magnitude = divisor.unsigned_abs();
    (magnitude >= 2 && magnitude.is_power_of_two()).then(|| (magnitude - 1) as i64)
}

fn emit_scalar_straight_loop(
    config: &NativeStraightLongLoopConfig,
    checked: bool,
    budgeted: bool,
    polling_interval: Option<u16>,
) -> Result<Box<[u8]>, X86StraightLongLoopError> {
    debug_assert!(!(budgeted && polling_interval.is_some()));
    let mut assembler = X86_64Assembler::new();
    let slots = X86_64Register::RDI;
    // Keep the loop induction outside RAX/RDX so signed division can use its
    // architectural dividend and remainder pair without spilling loop state.
    let induction = X86_64Register::R11;
    let bound = X86_64Register::RCX;
    let lhs = X86_64Register::RAX;
    let rhs = X86_64Register::R8;
    let auxiliary = X86_64Register::R9;
    let polling_remaining = X86_64Register::R10;
    let displacement = |slot: u16| i32::from(slot) * 8;

    assembler.move_from_base_disp32(induction, slots, displacement(config.induction_slot));
    emit_linear_operand(
        &mut assembler,
        config.bound,
        bound,
        config.induction_slot,
        induction,
    );
    if let Some(interval) = polling_interval {
        assembler.move_immediate64(polling_remaining, i64::from(interval));
    }
    assembler.compare_register(induction, bound);
    let completed_jump = assembler.jump_greater_or_equal_rel32();
    let loop_start = assembler.bytes.len();
    let mut operation_side_exit_jumps = Vec::new();
    let mut structured_conditional_jumps = Vec::new();
    let mut structured_jumps = Vec::new();
    let mut operation_offsets = [0usize; super::NATIVE_STRAIGHT_LONG_MAX_OPERATIONS + 1];

    for (operation_index, operation) in config.operations[..config.operation_count as usize]
        .iter()
        .copied()
        .enumerate()
    {
        operation_offsets[operation_index] = assembler.bytes.len();
        let (kind, left, right, result, destination) = match operation {
            NativeStraightLongOperation::Move { source, result } => {
                emit_linear_operand(
                    &mut assembler,
                    source,
                    lhs,
                    config.induction_slot,
                    induction,
                );
                assembler.move_to_base_disp32(slots, lhs, displacement(result));
                continue;
            }
            NativeStraightLongOperation::Modulo {
                value,
                divisor,
                result,
            } => (
                ScalarLongOpKind::Modulo,
                value,
                QuickLongOperand::Const(divisor),
                result,
                None,
            ),
            NativeStraightLongOperation::Binary {
                kind,
                lhs,
                rhs,
                result,
            } => (kind, lhs, rhs, result, None),
            NativeStraightLongOperation::BinaryAssign {
                kind,
                lhs,
                rhs,
                result,
                destination,
            } => (kind, lhs, rhs, result, Some(destination)),
            NativeStraightLongOperation::Guard {
                kind,
                lhs: condition_lhs,
                rhs: condition_rhs,
                expected,
            } => {
                emit_linear_condition_operand(
                    &mut assembler,
                    condition_lhs,
                    lhs,
                    auxiliary,
                    config.induction_slot,
                    induction,
                );
                emit_linear_condition_operand(
                    &mut assembler,
                    condition_rhs,
                    rhs,
                    auxiliary,
                    config.induction_slot,
                    induction,
                );
                assembler.compare_register(lhs, rhs);
                operation_side_exit_jumps.push((
                    emit_guard_mismatch_jump(&mut assembler, kind, expected),
                    operation_index as u8,
                ));
                continue;
            }
            NativeStraightLongOperation::BranchUnless {
                kind,
                lhs: condition_lhs,
                rhs: condition_rhs,
                false_target,
            } => {
                emit_linear_condition_operand(
                    &mut assembler,
                    condition_lhs,
                    lhs,
                    auxiliary,
                    config.induction_slot,
                    induction,
                );
                emit_linear_condition_operand(
                    &mut assembler,
                    condition_rhs,
                    rhs,
                    auxiliary,
                    config.induction_slot,
                    induction,
                );
                assembler.compare_register(lhs, rhs);
                structured_conditional_jumps.push((
                    emit_false_condition_jump(&mut assembler, kind),
                    false_target,
                ));
                continue;
            }
            NativeStraightLongOperation::Jump { target } => {
                structured_jumps.push((assembler.jump_rel32(), target));
                continue;
            }
            _ => {
                return Err(X86StraightLongLoopError::UnsupportedConfig(
                    "x86 scalar loop operation is not lowered",
                ));
            }
        };
        emit_linear_operand(&mut assembler, left, lhs, config.induction_slot, induction);
        emit_linear_operand(&mut assembler, right, rhs, config.induction_slot, induction);
        match kind {
            ScalarLongOpKind::Add => assembler.add_register(lhs, rhs),
            ScalarLongOpKind::Subtract => assembler.subtract_register(lhs, rhs),
            ScalarLongOpKind::Multiply => assembler.multiply_register(lhs, rhs),
            ScalarLongOpKind::BitwiseXor => assembler.xor_register(lhs, rhs),
            ScalarLongOpKind::Modulo if matches!(right, QuickLongOperand::Const(divisor) if signed_power_of_two_remainder_mask(divisor).is_some()) =>
            {
                let QuickLongOperand::Const(divisor) = right else {
                    unreachable!();
                };
                let mask = signed_power_of_two_remainder_mask(divisor).unwrap();
                // Truncating signed remainder by 2^k without IDIV:
                // bias = sign(lhs) & mask; ((lhs + bias) & mask) - bias.
                assembler.move_register(auxiliary, lhs);
                assembler.arithmetic_shift_right_immediate8(auxiliary, 63);
                assembler.move_immediate64(rhs, mask);
                assembler.and_register(auxiliary, rhs);
                assembler.add_register(lhs, auxiliary);
                assembler.and_register(lhs, rhs);
                assembler.subtract_register(lhs, auxiliary);
            }
            ScalarLongOpKind::IntDivide | ScalarLongOpKind::Modulo => {
                if checked {
                    assembler.compare_immediate8(rhs, 0);
                    operation_side_exit_jumps
                        .push((assembler.jump_equal_rel32(), operation_index as u8));
                    assembler.compare_immediate8(rhs, -1);
                    let safe_divisor = assembler.jump_not_equal_rel32();
                    assembler.move_immediate64(auxiliary, i64::MIN);
                    assembler.compare_register(lhs, auxiliary);
                    operation_side_exit_jumps
                        .push((assembler.jump_equal_rel32(), operation_index as u8));
                    let divide = assembler.bytes.len();
                    assembler.patch_rel32(safe_divisor, divide);
                }
                assembler.sign_extend_rax_into_rdx();
                assembler.signed_divide(rhs);
                if kind == ScalarLongOpKind::Modulo {
                    assembler.move_register(lhs, X86_64Register::RDX);
                }
            }
        }
        if checked
            && matches!(
                kind,
                ScalarLongOpKind::Add | ScalarLongOpKind::Subtract | ScalarLongOpKind::Multiply
            )
        {
            operation_side_exit_jumps
                .push((assembler.jump_overflow_rel32(), operation_index as u8));
        }
        assembler.move_to_base_disp32(slots, lhs, displacement(result));
        if let Some(destination) = destination
            && destination != result
        {
            assembler.move_to_base_disp32(slots, lhs, displacement(destination));
        }
    }
    operation_offsets[config.operation_count as usize] = assembler.bytes.len();
    for (branch, target) in structured_conditional_jumps {
        assembler.patch_rel32(branch, operation_offsets[target as usize]);
    }
    for (branch, target) in structured_jumps {
        assembler.patch_rel32(branch, operation_offsets[target as usize]);
    }

    if let Some(post_result) = config.post_result {
        assembler.move_to_base_disp32(slots, induction, displacement(post_result));
    }
    assembler.add_immediate8(induction, 1);
    assembler.compare_register(induction, bound);
    let completed_after_iteration_jump =
        (budgeted || polling_interval.is_some()).then(|| assembler.jump_greater_or_equal_rel32());
    let mut loop_jumps = Vec::with_capacity(2);
    let interrupt_jump = if budgeted {
        assembler.subtract_immediate8(X86_64Register::RSI, 1);
        loop_jumps.push(assembler.jump_not_equal_rel32());
        None
    } else if let Some(interval) = polling_interval {
        assembler.subtract_immediate8(polling_remaining, 1);
        loop_jumps.push(assembler.jump_not_equal_rel32());
        assembler.compare_byte_base_immediate8(X86_64Register::RSI, 0);
        let interrupt_jump = assembler.jump_not_equal_rel32();
        assembler.move_immediate64(polling_remaining, i64::from(interval));
        loop_jumps.push(assembler.jump_rel32());
        Some(interrupt_jump)
    } else {
        loop_jumps.push(assembler.jump_less_than_rel32());
        None
    };

    if budgeted || polling_interval.is_some() {
        let chunk_exhausted = assembler.bytes.len();
        if let Some(interrupt_jump) = interrupt_jump {
            assembler.patch_rel32(interrupt_jump, chunk_exhausted);
        }
        assembler.move_to_base_disp32(slots, induction, displacement(config.induction_slot));
        assembler.move_immediate32_eax(X86_STRAIGHT_CHUNK_EXHAUSTED);
        assembler.return_near();
    }

    let completed = assembler.bytes.len();
    assembler.patch_rel32(completed_jump, completed);
    if let Some(completed_after_iteration_jump) = completed_after_iteration_jump {
        assembler.patch_rel32(completed_after_iteration_jump, completed);
    }
    for loop_jump in loop_jumps {
        assembler.patch_rel32(loop_jump, loop_start);
    }
    assembler.move_to_base_disp32(slots, induction, displacement(config.induction_slot));
    assembler.clear_eax();
    assembler.return_near();

    for (side_exit_jump, operation_index) in operation_side_exit_jumps {
        let side_exit = assembler.bytes.len();
        assembler.patch_rel32(side_exit_jump, side_exit);
        assembler.move_to_base_disp32(slots, induction, displacement(config.induction_slot));
        let status = X86_STRAIGHT_OPERATION_SIDE_EXIT | (u32::from(operation_index) << 8);
        assembler.move_immediate32_eax(status);
        assembler.return_near();
    }

    Ok(assembler.finish())
}

fn validate_scalar_straight_config(
    config: &NativeStraightLongLoopConfig,
) -> Result<(), X86StraightLongLoopError> {
    let validate_slot = |slot: u16| {
        if slot < 64 {
            Ok(())
        } else {
            Err(X86StraightLongLoopError::UnsupportedConfig(
                "x86 straight-loop slot exceeds the fixed shadow",
            ))
        }
    };
    let validate_operand = |operand: QuickLongOperand| match operand {
        QuickLongOperand::Slot(slot) => validate_slot(slot),
        QuickLongOperand::Const(_) => Ok(()),
    };
    let validate_condition_operand = |operand: NativeStraightLongConditionOperand| match operand {
        NativeStraightLongConditionOperand::Source(source) => validate_operand(source),
        NativeStraightLongConditionOperand::BitwiseAnd { lhs, rhs } => {
            validate_operand(lhs)?;
            validate_operand(rhs)
        }
    };
    let validate_output = |slot: u16| {
        validate_slot(slot)?;
        if slot == config.induction_slot {
            Err(X86StraightLongLoopError::UnsupportedConfig(
                "x86 scalar loop body cannot overwrite its induction slot",
            ))
        } else {
            Ok(())
        }
    };

    validate_slot(config.induction_slot)?;
    validate_operand(config.bound)?;
    if let Some(post_result) = config.post_result {
        validate_output(post_result)?;
    }
    let mut written_mask = 1u64 << config.induction_slot;
    if let Some(post_result) = config.post_result {
        written_mask |= 1u64 << post_result;
    }
    for (index, operation) in config.operations[..config.operation_count as usize]
        .iter()
        .copied()
        .enumerate()
    {
        match operation {
            NativeStraightLongOperation::Modulo {
                value,
                divisor: _,
                result,
            } => {
                validate_operand(value)?;
                validate_output(result)?;
                written_mask |= 1u64 << result;
            }
            NativeStraightLongOperation::Move { source, result } => {
                validate_operand(source)?;
                validate_output(result)?;
                written_mask |= 1u64 << result;
            }
            NativeStraightLongOperation::Binary {
                kind: _,
                lhs,
                rhs,
                result,
            } => {
                validate_operand(lhs)?;
                validate_operand(rhs)?;
                validate_output(result)?;
                written_mask |= 1u64 << result;
            }
            NativeStraightLongOperation::BinaryAssign {
                kind: _,
                lhs,
                rhs,
                result,
                destination,
            } => {
                validate_operand(lhs)?;
                validate_operand(rhs)?;
                validate_output(result)?;
                validate_output(destination)?;
                written_mask |= (1u64 << result) | (1u64 << destination);
            }
            NativeStraightLongOperation::Guard { lhs, rhs, .. } => {
                validate_condition_operand(lhs)?;
                validate_condition_operand(rhs)?;
            }
            NativeStraightLongOperation::BranchUnless {
                lhs,
                rhs,
                false_target,
                ..
            } => {
                validate_condition_operand(lhs)?;
                validate_condition_operand(rhs)?;
                if false_target as usize <= index
                    || false_target as usize > config.operation_count as usize
                {
                    return Err(X86StraightLongLoopError::UnsupportedConfig(
                        "x86 conditional branch target is not forward and in range",
                    ));
                }
            }
            NativeStraightLongOperation::Jump { target } => {
                if target as usize <= index || target as usize > config.operation_count as usize {
                    return Err(X86StraightLongLoopError::UnsupportedConfig(
                        "x86 jump target is not forward and in range",
                    ));
                }
            }
            _ => {
                return Err(X86StraightLongLoopError::UnsupportedConfig(
                    "x86 scalar loop operation is not lowered",
                ));
            }
        }
    }
    if matches!(config.bound, QuickLongOperand::Slot(slot) if written_mask & (1u64 << slot) != 0) {
        return Err(X86StraightLongLoopError::UnsupportedConfig(
            "x86 straight-loop bound cannot be written by the loop",
        ));
    }
    Ok(())
}

impl CompiledX86StraightLongLoop {
    pub fn compile(config: NativeStraightLongLoopConfig) -> Result<Self, X86StraightLongLoopError> {
        if config.operation_count == 0
            || config.operation_count as usize > super::NATIVE_STRAIGHT_LONG_MAX_OPERATIONS
        {
            return Err(X86StraightLongLoopError::UnsupportedConfig(
                "x86 straight-loop operation count is outside the shared IR capacity",
            ));
        }
        let bound = config.bound;
        let induction = config.induction_slot;
        let additive_recurrence = if config.operation_count == 1 {
            match config.operations[0] {
                NativeStraightLongOperation::BinaryAssign {
                    kind: ScalarLongOpKind::Add,
                    lhs: QuickLongOperand::Slot(lhs),
                    rhs: QuickLongOperand::Slot(rhs),
                    result,
                    destination,
                } if lhs == destination && rhs == induction => Some((lhs, result, destination)),
                NativeStraightLongOperation::BinaryAssign {
                    kind: ScalarLongOpKind::Add,
                    lhs: QuickLongOperand::Slot(lhs),
                    rhs: QuickLongOperand::Slot(rhs),
                    result,
                    destination,
                } if rhs == destination && lhs == induction => Some((rhs, result, destination)),
                _ => None,
            }
        } else {
            None
        };
        let Some((accumulator, result, destination)) = additive_recurrence else {
            return Self::compile_scalar(config);
        };
        let bound_slot = match bound {
            QuickLongOperand::Slot(slot) => Some(slot),
            QuickLongOperand::Const(_) => None,
        };
        if bound_slot.is_some_and(|slot| slot >= 64) {
            return Err(X86StraightLongLoopError::UnsupportedConfig(
                "x86 straight-loop slot exceeds the fixed shadow",
            ));
        }
        if bound_slot.is_some_and(|slot| config.body_output_mask() & (1u64 << slot) != 0) {
            return Err(X86StraightLongLoopError::UnsupportedConfig(
                "x86 straight-loop bound cannot be written by the loop body",
            ));
        }
        for slot in [induction, accumulator, result, destination]
            .into_iter()
            .chain(bound_slot)
        {
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
            None,
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
            None,
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
            None,
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
            None,
        );
        code.extend_from_slice(&checked_chunk_code);
        let polling_entry_offset = code.len();
        let polling_code = emit_additive_recurrence_loop(
            induction,
            accumulator,
            result,
            destination,
            bound,
            false,
            false,
            Some(X86_STRAIGHT_SAFEPOINT_INTERVAL),
        );
        code.extend_from_slice(&polling_code);
        let code = code.into_boxed_slice();
        let memory = ExecutableMemory::from_code(&code)?;
        Ok(Self {
            memory,
            code,
            config,
            checked_entry_offset,
            chunk_entry_offset,
            checked_chunk_entry_offset,
            polling_entry_offset,
        })
    }

    fn compile_scalar(
        config: NativeStraightLongLoopConfig,
    ) -> Result<Self, X86StraightLongLoopError> {
        validate_scalar_straight_config(&config)?;
        let fast_code = emit_scalar_straight_loop(&config, false, false, None)?;
        let checked_entry_offset = fast_code.len();
        let checked_code = emit_scalar_straight_loop(&config, true, false, None)?;
        let mut code = fast_code.into_vec();
        code.extend_from_slice(&checked_code);
        let chunk_entry_offset = code.len();
        let chunk_code = emit_scalar_straight_loop(&config, false, true, None)?;
        code.extend_from_slice(&chunk_code);
        let checked_chunk_entry_offset = code.len();
        let checked_chunk_code = emit_scalar_straight_loop(&config, true, true, None)?;
        code.extend_from_slice(&checked_chunk_code);
        let polling_entry_offset = code.len();
        let polling_code = emit_scalar_straight_loop(
            &config,
            false,
            false,
            Some(X86_STRAIGHT_SAFEPOINT_INTERVAL),
        )?;
        code.extend_from_slice(&polling_code);
        let code = code.into_boxed_slice();
        let memory = ExecutableMemory::from_code(&code)?;
        Ok(Self {
            memory,
            code,
            config,
            checked_entry_offset,
            chunk_entry_offset,
            checked_chunk_entry_offset,
            polling_entry_offset,
        })
    }

    pub fn call(
        &self,
        slots: &mut [i64; 64],
    ) -> Result<NativeStraightLongLoopResult, X86StraightLongLoopError> {
        if slots[self.config.induction_slot as usize] >= self.bound_value(slots) {
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
        if slots[self.config.induction_slot as usize] >= self.bound_value(slots) {
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
        self.call_chunk_entry(slots, iteration_budget, entry_offset)
    }

    fn call_proven_polling(
        &self,
        slots: &mut [i64; 64],
        interrupt_flag: *const bool,
    ) -> Result<NativeStraightLongLoopResult, X86StraightLongLoopError> {
        if slots[self.config.induction_slot as usize] >= self.bound_value(slots) {
            return Ok(NativeStraightLongLoopResult {
                outcome: NativeStraightLongLoopOutcome::Completed,
                failed_operation: None,
            });
        }
        let entry = unsafe { self.memory.entry().add(self.polling_entry_offset) };
        type NativeFunction = unsafe extern "C" fn(*mut i64, *const bool) -> u32;
        let function: NativeFunction = unsafe { std::mem::transmute(entry) };
        let status = unsafe { function(slots.as_mut_ptr(), interrupt_flag) };
        self.decode_status(status)
    }

    fn call_chunk_entry(
        &self,
        slots: &mut [i64; 64],
        iteration_budget: u64,
        entry_offset: usize,
    ) -> Result<NativeStraightLongLoopResult, X86StraightLongLoopError> {
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
            status if status & 0xff == X86_STRAIGHT_OPERATION_SIDE_EXIT => {
                let operation = u8::try_from(status >> 8)
                    .map_err(|_| X86StraightLongLoopError::InvalidStatus(status))?;
                if operation >= self.config.operation_count {
                    return Err(X86StraightLongLoopError::InvalidStatus(status));
                }
                Ok(NativeStraightLongLoopResult {
                    outcome: NativeStraightLongLoopOutcome::OperationSideExit,
                    failed_operation: Some(operation),
                })
            }
            status => Err(X86StraightLongLoopError::InvalidStatus(status)),
        }
    }

    pub fn code(&self) -> &[u8] {
        &self.code
    }

    pub fn config(&self) -> NativeStraightLongLoopConfig {
        self.config
    }

    fn bound_value(&self, slots: &[i64; 64]) -> i64 {
        match self.config.bound {
            QuickLongOperand::Slot(slot) => slots[slot as usize],
            QuickLongOperand::Const(bound) => bound,
        }
    }
}

/// Per-quick-region x86 cache. Unsupported shared IR stays uncompiled and the
/// caller continues through the canonical typed executor.
pub struct X86QuickLongOpsJitCache {
    straight_compiled: OnceCell<Option<CompiledX86StraightLongLoop>>,
    native_entries: Cell<u64>,
    native_calls: Cell<u64>,
    native_chunks: Cell<u64>,
    range_proven_chunks: Cell<u64>,
    range_proof_evaluations: Cell<u64>,
    side_exits: Cell<u64>,
}

impl X86QuickLongOpsJitCache {
    pub const fn new() -> Self {
        Self {
            straight_compiled: OnceCell::new(),
            native_entries: Cell::new(0),
            native_calls: Cell::new(0),
            native_chunks: Cell::new(0),
            range_proven_chunks: Cell::new(0),
            range_proof_evaluations: Cell::new(0),
            side_exits: Cell::new(0),
        }
    }

    pub(crate) fn prove_straight_remaining_range(
        &self,
        config: &NativeStraightLongLoopConfig,
        slots: &[i64; 64],
    ) -> Option<super::straight::StraightLongRangeProof> {
        self.range_proof_evaluations
            .set(self.range_proof_evaluations.get().saturating_add(1));
        straight_long_remaining_range_proof(config, slots)
    }

    pub fn prepare_straight_program(
        &self,
        config: &NativeStraightLongLoopConfig,
    ) -> Option<&CompiledX86StraightLongLoop> {
        let program = self
            .straight_compiled
            .get_or_init(|| CompiledX86StraightLongLoop::compile(*config).ok())
            .as_ref()?;
        (program.config() == *config).then_some(program)
    }

    pub(crate) fn prepare_range_proven_straight_program(
        &self,
        config: &NativeStraightLongLoopConfig,
        _safepoint_interval: u16,
        _publication_mask: u64,
        _carried_mask: u64,
    ) -> Option<&CompiledX86StraightLongLoop> {
        self.prepare_straight_program(config)
    }

    pub(crate) fn dispatch_prepared_proven_straight_remaining(
        &self,
        program: &CompiledX86StraightLongLoop,
        config: &NativeStraightLongLoopConfig,
        slots: &mut [i64; 64],
        _interrupt_flag: *const bool,
        safepoint_interval: u16,
    ) -> Option<Result<NativeStraightLongLoopResult, X86StraightLongLoopError>> {
        if safepoint_interval != X86_STRAIGHT_SAFEPOINT_INTERVAL {
            return None;
        }
        if slots[config.induction_slot as usize] >= program.bound_value(slots) {
            return None;
        }
        let before_induction = slots[config.induction_slot as usize];
        self.native_calls
            .set(self.native_calls.get().saturating_add(1));
        let outcome = program.call_proven_polling(slots, _interrupt_flag);
        let completed_iterations =
            (slots[config.induction_slot as usize] as u64).wrapping_sub(before_induction as u64);
        let completed_chunks = completed_iterations
            .div_ceil(u64::from(safepoint_interval))
            .max(1);
        self.native_chunks
            .set(self.native_chunks.get().saturating_add(completed_chunks));
        self.range_proven_chunks.set(
            self.range_proven_chunks
                .get()
                .saturating_add(completed_chunks),
        );
        self.record_side_exit(&outcome);
        Some(outcome)
    }

    pub fn dispatch_prepared_straight_chunk(
        &self,
        program: &CompiledX86StraightLongLoop,
        slots: &mut [i64; 64],
        iteration_budget: u64,
    ) -> Result<NativeStraightLongLoopResult, X86StraightLongLoopError> {
        self.native_calls
            .set(self.native_calls.get().saturating_add(1));
        self.native_chunks
            .set(self.native_chunks.get().saturating_add(1));
        let outcome = program.call_chunk(slots, iteration_budget);
        self.record_side_exit(&outcome);
        outcome
    }

    fn record_side_exit(
        &self,
        outcome: &Result<NativeStraightLongLoopResult, X86StraightLongLoopError>,
    ) {
        if matches!(
            outcome,
            Ok(NativeStraightLongLoopResult {
                outcome: NativeStraightLongLoopOutcome::OperationSideExit
                    | NativeStraightLongLoopOutcome::IncrementOverflow,
                ..
            }) | Err(_)
        ) {
            self.side_exits.set(self.side_exits.get().saturating_add(1));
        }
    }

    pub fn record_region_entry(&self) {
        self.native_entries
            .set(self.native_entries.get().saturating_add(1));
    }

    pub fn is_straight_compiled(&self) -> bool {
        matches!(self.straight_compiled.get(), Some(Some(_)))
    }

    pub fn native_entries(&self) -> u64 {
        self.native_entries.get()
    }

    pub fn native_calls(&self) -> u64 {
        self.native_calls.get()
    }

    pub fn native_chunks(&self) -> u64 {
        self.native_chunks.get()
    }

    pub fn range_proven_chunks(&self) -> u64 {
        self.range_proven_chunks.get()
    }

    pub fn range_proof_evaluations(&self) -> u64 {
        self.range_proof_evaluations.get()
    }

    pub fn side_exits(&self) -> u64 {
        self.side_exits.get()
    }
}

impl Default for X86QuickLongOpsJitCache {
    fn default() -> Self {
        Self::new()
    }
}

impl Clone for X86QuickLongOpsJitCache {
    fn clone(&self) -> Self {
        Self::new()
    }
}

impl fmt::Debug for X86QuickLongOpsJitCache {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter
            .debug_struct("X86QuickLongOpsJitCache")
            .field("compiled", &self.is_straight_compiled())
            .field("native_entries", &self.native_entries())
            .field("native_calls", &self.native_calls())
            .field("native_chunks", &self.native_chunks())
            .field("side_exits", &self.side_exits())
            .finish()
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

    fn composed_add_recurrence(bound: i64) -> NativeStraightLongLoopConfig {
        let mut operations =
            [NativeStraightLongOperation::Unused; NATIVE_STRAIGHT_LONG_MAX_OPERATIONS];
        operations[0] = NativeStraightLongOperation::Binary {
            kind: ScalarLongOpKind::Add,
            lhs: QuickLongOperand::Slot(0),
            rhs: QuickLongOperand::Const(1),
            result: 4,
        };
        operations[1] = NativeStraightLongOperation::BinaryAssign {
            kind: ScalarLongOpKind::Add,
            lhs: QuickLongOperand::Slot(1),
            rhs: QuickLongOperand::Slot(4),
            result: 2,
            destination: 1,
        };
        NativeStraightLongLoopConfig {
            induction_slot: 0,
            bound: QuickLongOperand::Const(bound),
            operations,
            operation_count: 2,
            post_result: Some(5),
        }
    }

    fn structured_recurrence(bound: i64) -> NativeStraightLongLoopConfig {
        let mut operations =
            [NativeStraightLongOperation::Unused; NATIVE_STRAIGHT_LONG_MAX_OPERATIONS];
        operations[0] = NativeStraightLongOperation::BranchUnless {
            kind: ScalarLongConditionKind::LessThan,
            lhs: NativeStraightLongConditionOperand::Source(QuickLongOperand::Slot(0)),
            rhs: NativeStraightLongConditionOperand::Source(QuickLongOperand::Const(2)),
            false_target: 3,
        };
        operations[1] = NativeStraightLongOperation::BinaryAssign {
            kind: ScalarLongOpKind::Add,
            lhs: QuickLongOperand::Slot(1),
            rhs: QuickLongOperand::Const(10),
            result: 2,
            destination: 1,
        };
        operations[2] = NativeStraightLongOperation::Jump { target: 4 };
        operations[3] = NativeStraightLongOperation::BinaryAssign {
            kind: ScalarLongOpKind::Add,
            lhs: QuickLongOperand::Slot(1),
            rhs: QuickLongOperand::Const(100),
            result: 2,
            destination: 1,
        };
        operations[4] = NativeStraightLongOperation::BinaryAssign {
            kind: ScalarLongOpKind::Add,
            lhs: QuickLongOperand::Slot(1),
            rhs: QuickLongOperand::Const(1),
            result: 2,
            destination: 1,
        };
        NativeStraightLongLoopConfig {
            induction_slot: 0,
            bound: QuickLongOperand::Const(bound),
            operations,
            operation_count: 5,
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
    fn dynamic_bound_is_loaded_from_shadow_on_every_native_entry() {
        let mut config = additive_recurrence(0, false);
        config.bound = QuickLongOperand::Slot(3);
        let program = CompiledX86StraightLongLoop::compile(config).unwrap();

        let mut first = [0_i64; 64];
        first[1] = 10;
        first[3] = 4;
        program.call(&mut first).unwrap();
        assert_eq!(&first[..4], &[4, 16, 16, 4]);

        let mut second = [0_i64; 64];
        second[1] = 1;
        second[3] = 6;
        program.call(&mut second).unwrap();
        assert_eq!(&second[..4], &[6, 16, 16, 6]);
    }

    #[test]
    fn linear_lowering_executes_composed_operations_and_post_result() {
        let program = CompiledX86StraightLongLoop::compile(composed_add_recurrence(4)).unwrap();
        let mut slots = [0_i64; 64];
        slots[1] = 10;
        let result = program.call(&mut slots).unwrap();
        assert_eq!(result.outcome, NativeStraightLongLoopOutcome::Completed);
        assert_eq!(&slots[..6], &[4, 20, 20, 0, 4, 3]);
    }

    #[test]
    fn linear_lowering_supports_subtract_and_multiply() {
        let mut config = composed_add_recurrence(3);
        config.operations[0] = NativeStraightLongOperation::BinaryAssign {
            kind: ScalarLongOpKind::Multiply,
            lhs: QuickLongOperand::Slot(1),
            rhs: QuickLongOperand::Const(2),
            result: 2,
            destination: 1,
        };
        config.operations[1] = NativeStraightLongOperation::Binary {
            kind: ScalarLongOpKind::Subtract,
            lhs: QuickLongOperand::Slot(1),
            rhs: QuickLongOperand::Const(3),
            result: 4,
        };
        config.post_result = None;
        let program = CompiledX86StraightLongLoop::compile(config).unwrap();
        let mut slots = [0_i64; 64];
        slots[1] = 2;
        let result = program.call(&mut slots).unwrap();
        assert_eq!(result.outcome, NativeStraightLongLoopOutcome::Completed);
        assert_eq!(&slots[..5], &[3, 16, 16, 0, 13]);
    }

    #[test]
    fn linear_checked_exit_reports_exact_failed_operation() {
        let mut config = composed_add_recurrence(1);
        config.operations[0] = NativeStraightLongOperation::Move {
            source: QuickLongOperand::Const(2),
            result: 4,
        };
        config.operations[1] = NativeStraightLongOperation::BinaryAssign {
            kind: ScalarLongOpKind::Add,
            lhs: QuickLongOperand::Slot(1),
            rhs: QuickLongOperand::Slot(4),
            result: 2,
            destination: 1,
        };
        config.post_result = None;
        let program = CompiledX86StraightLongLoop::compile(config).unwrap();
        let mut slots = [0_i64; 64];
        slots[1] = i64::MAX;
        slots[2] = 77;
        let result = program.call(&mut slots).unwrap();
        assert_eq!(
            result,
            NativeStraightLongLoopResult {
                outcome: NativeStraightLongLoopOutcome::OperationSideExit,
                failed_operation: Some(1),
            }
        );
        assert_eq!(&slots[..5], &[0, i64::MAX, 77, 0, 2]);
    }

    #[test]
    fn linear_polling_entry_preserves_composed_state_at_safepoint() {
        let program = CompiledX86StraightLongLoop::compile(composed_add_recurrence(5_000)).unwrap();
        let mut slots = [0_i64; 64];
        slots[1] = 10;
        let interrupt = true;
        let result = program.call_proven_polling(&mut slots, &interrupt).unwrap();
        assert_eq!(
            result.outcome,
            NativeStraightLongLoopOutcome::ChunkExhausted
        );
        assert_eq!(slots[0], 1_024);
        assert_eq!(slots[1], 524_810);
        assert_eq!(slots[2], 524_810);
        assert_eq!(slots[4], 1_024);
        assert_eq!(slots[5], 1_023);
    }

    #[test]
    fn structured_lowering_executes_both_forward_control_flow_edges() {
        let program = CompiledX86StraightLongLoop::compile(structured_recurrence(4)).unwrap();
        let mut slots = [0_i64; 64];
        let result = program.call(&mut slots).unwrap();
        assert_eq!(result.outcome, NativeStraightLongLoopOutcome::Completed);
        assert_eq!(&slots[..3], &[4, 224, 224]);
    }

    #[test]
    fn structured_bitwise_condition_executes_in_private_shadow() {
        let mut config = structured_recurrence(4);
        config.operations[0] = NativeStraightLongOperation::BranchUnless {
            kind: ScalarLongConditionKind::Equal,
            lhs: NativeStraightLongConditionOperand::BitwiseAnd {
                lhs: QuickLongOperand::Slot(0),
                rhs: QuickLongOperand::Const(1),
            },
            rhs: NativeStraightLongConditionOperand::Source(QuickLongOperand::Const(0)),
            false_target: 3,
        };
        let program = CompiledX86StraightLongLoop::compile(config).unwrap();
        let mut slots = [0_i64; 64];
        let result = program.call(&mut slots).unwrap();
        assert_eq!(result.outcome, NativeStraightLongLoopOutcome::Completed);
        assert_eq!(&slots[..3], &[4, 224, 224]);
    }

    #[test]
    fn guard_side_exit_reports_exact_operation_after_completed_iterations() {
        let mut config = structured_recurrence(4);
        config.operations[0] = NativeStraightLongOperation::Guard {
            kind: ScalarLongConditionKind::LessThan,
            lhs: NativeStraightLongConditionOperand::Source(QuickLongOperand::Slot(0)),
            rhs: NativeStraightLongConditionOperand::Source(QuickLongOperand::Const(2)),
            expected: true,
        };
        config.operations[1] = NativeStraightLongOperation::BinaryAssign {
            kind: ScalarLongOpKind::Add,
            lhs: QuickLongOperand::Slot(1),
            rhs: QuickLongOperand::Const(10),
            result: 2,
            destination: 1,
        };
        config.operations[2] = NativeStraightLongOperation::Jump { target: 5 };
        let program = CompiledX86StraightLongLoop::compile(config).unwrap();
        let mut slots = [0_i64; 64];
        let result = program.call(&mut slots).unwrap();
        assert_eq!(
            result,
            NativeStraightLongLoopResult {
                outcome: NativeStraightLongLoopOutcome::OperationSideExit,
                failed_operation: Some(0),
            }
        );
        assert_eq!(&slots[..3], &[2, 20, 20]);
    }

    #[test]
    fn scalar_lowering_executes_divide_modulo_and_xor() {
        let mut config = composed_add_recurrence(5);
        config.operations[0] = NativeStraightLongOperation::Binary {
            kind: ScalarLongOpKind::Modulo,
            lhs: QuickLongOperand::Slot(0),
            rhs: QuickLongOperand::Const(3),
            result: 4,
        };
        config.operations[1] = NativeStraightLongOperation::Binary {
            kind: ScalarLongOpKind::IntDivide,
            lhs: QuickLongOperand::Slot(0),
            rhs: QuickLongOperand::Const(2),
            result: 5,
        };
        config.operations[2] = NativeStraightLongOperation::Binary {
            kind: ScalarLongOpKind::BitwiseXor,
            lhs: QuickLongOperand::Slot(4),
            rhs: QuickLongOperand::Slot(5),
            result: 6,
        };
        config.operation_count = 3;
        config.post_result = None;
        let program = CompiledX86StraightLongLoop::compile(config).unwrap();
        let mut slots = [0_i64; 64];
        let result = program.call(&mut slots).unwrap();
        assert_eq!(result.outcome, NativeStraightLongLoopOutcome::Completed);
        assert_eq!(slots[0], 5);
        assert_eq!(&slots[4..7], &[1, 2, 3]);
    }

    #[test]
    fn checked_division_side_exit_prevents_native_zero_divide() {
        let mut config = composed_add_recurrence(1);
        config.operations[0] = NativeStraightLongOperation::Binary {
            kind: ScalarLongOpKind::IntDivide,
            lhs: QuickLongOperand::Slot(0),
            rhs: QuickLongOperand::Slot(7),
            result: 4,
        };
        config.operation_count = 1;
        config.post_result = None;
        let program = CompiledX86StraightLongLoop::compile(config).unwrap();
        let mut slots = [0_i64; 64];
        slots[4] = 91;
        slots[7] = 0;
        let result = program.call(&mut slots).unwrap();
        assert_eq!(
            result,
            NativeStraightLongLoopResult {
                outcome: NativeStraightLongLoopOutcome::OperationSideExit,
                failed_operation: Some(0),
            }
        );
        assert_eq!(slots[0], 0);
        assert_eq!(slots[4], 91);
    }

    #[test]
    fn standalone_modulo_preserves_signed_remainder_semantics() {
        let mut config = composed_add_recurrence(1);
        config.operations[0] = NativeStraightLongOperation::Modulo {
            value: QuickLongOperand::Slot(6),
            divisor: 2,
            result: 4,
        };
        config.operation_count = 1;
        config.post_result = None;
        let program = CompiledX86StraightLongLoop::compile(config).unwrap();
        let mut slots = [0_i64; 64];
        slots[6] = -5;
        let result = program.call(&mut slots).unwrap();
        assert_eq!(result.outcome, NativeStraightLongLoopOutcome::Completed);
        assert_eq!(slots[4], -1);
    }

    #[test]
    fn modulo_conditional_accumulate_matches_quick_ops_shape() {
        let mut operations =
            [NativeStraightLongOperation::Unused; NATIVE_STRAIGHT_LONG_MAX_OPERATIONS];
        operations[0] = NativeStraightLongOperation::Modulo {
            value: QuickLongOperand::Slot(2),
            divisor: 2,
            result: 4,
        };
        operations[1] = NativeStraightLongOperation::BranchUnless {
            kind: ScalarLongConditionKind::Equal,
            lhs: NativeStraightLongConditionOperand::Source(QuickLongOperand::Slot(4)),
            rhs: NativeStraightLongConditionOperand::Source(QuickLongOperand::Const(0)),
            false_target: 3,
        };
        operations[2] = NativeStraightLongOperation::BinaryAssign {
            kind: ScalarLongOpKind::Add,
            lhs: QuickLongOperand::Slot(1),
            rhs: QuickLongOperand::Slot(2),
            result: 6,
            destination: 1,
        };
        let config = NativeStraightLongLoopConfig {
            induction_slot: 2,
            bound: QuickLongOperand::Slot(0),
            operations,
            operation_count: 3,
            post_result: None,
        };
        let program = CompiledX86StraightLongLoop::compile(config).unwrap();
        let mut slots = [0_i64; 64];
        slots[0] = 100_000;
        let result = program.call(&mut slots).unwrap();
        assert_eq!(result.outcome, NativeStraightLongLoopOutcome::Completed);
        assert_eq!(slots[2], 100_000);
        assert_eq!(slots[1], 2_499_950_000);
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
    fn polling_entry_stays_native_until_interrupt_or_completion() {
        let program =
            CompiledX86StraightLongLoop::compile(additive_recurrence(5_000, false)).unwrap();
        let mut slots = [0_i64; 64];
        slots[1] = 5;
        let interrupt = true;
        let interrupted = program.call_proven_polling(&mut slots, &interrupt).unwrap();
        assert_eq!(
            interrupted,
            NativeStraightLongLoopResult {
                outcome: NativeStraightLongLoopOutcome::ChunkExhausted,
                failed_operation: None,
            }
        );
        assert_eq!(&slots[..3], &[1_024, 523_781, 523_781]);

        let interrupt = false;
        let completed = program.call_proven_polling(&mut slots, &interrupt).unwrap();
        assert_eq!(completed.outcome, NativeStraightLongLoopOutcome::Completed);
        assert_eq!(&slots[..3], &[5_000, 12_497_505, 12_497_505]);
    }

    #[test]
    fn polling_entry_gives_completion_priority_over_pending_interrupt() {
        let program =
            CompiledX86StraightLongLoop::compile(additive_recurrence(100, false)).unwrap();
        let mut slots = [0_i64; 64];
        slots[1] = 5;
        let interrupt = true;
        let result = program.call_proven_polling(&mut slots, &interrupt).unwrap();
        assert_eq!(result.outcome, NativeStraightLongLoopOutcome::Completed);
        assert_eq!(&slots[..3], &[100, 4_955, 4_955]);
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
