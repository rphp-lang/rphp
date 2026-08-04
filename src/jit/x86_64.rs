//! Minimal x86-64 SysV backend slice.
//!
//! Like the ARM64 backend, this encoder writes machine instructions directly;
//! it does not invoke an assembler, linker or external code-generation crate.

use super::memory::ExecutableMemory;
use super::straight::{
    NativeStraightLongConditionOperand, NativeStraightLongLoopConfig,
    NativeStraightLongLoopOutcome, NativeStraightLongLoopResult, NativeStraightLongOperation,
    straight_long_best_invariant_slot_masks, straight_long_linear_live_after,
    straight_long_linear_shadow_store_mask, straight_long_operation_input_mask,
    straight_long_remaining_range_proof, straight_long_structured_block_starts,
    straight_long_structured_definitely_written,
    straight_long_structured_local_resident_output_masks,
};
use crate::vm::function::{
    ScalarLongConditionKind, ScalarLongConditionOperand, ScalarLongFunctionPlan, ScalarLongOp,
    ScalarLongOpKind, ScalarLongSource,
};
use crate::vm::quick::QuickLongOperand;
use std::cell::{Cell, OnceCell};
use std::fmt;
use std::io;
use std::mem::MaybeUninit;

#[path = "x86_64_branch.rs"]
mod branch;

const X86_STRAIGHT_COMPLETED: u32 = 0;
const X86_STRAIGHT_CHUNK_EXHAUSTED: u32 = 1;
const X86_STRAIGHT_OPERATION_SIDE_EXIT: u32 = 6;
const X86_STRAIGHT_SAFEPOINT_INTERVAL: u16 = 1024;
// Keep the hot structured body inside a fresh L1-I cache line when possible.
// The entry guard executes once; the aligned body is revisited on every
// iteration. Linear bodies remain compact and do not pay this padding.
const X86_STRUCTURED_LOOP_ALIGNMENT: usize = 64;

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
    const R12: Self = Self(12);
    const R13: Self = Self(13);
    const R14: Self = Self(14);
    const R15: Self = Self(15);

    #[inline]
    const fn low_bits(self) -> u8 {
        self.0 & 7
    }

    #[inline]
    const fn extension(self) -> u8 {
        (self.0 >> 3) & 1
    }
}

#[derive(Debug, Clone, Copy)]
struct X86BranchFixup {
    instruction: usize,
    displacement: usize,
    target: Option<usize>,
    short_opcode: u8,
    near_length: usize,
    relaxable: bool,
}

impl X86BranchFixup {
    #[inline]
    const fn saved_bytes(self) -> usize {
        self.near_length - 2
    }
}

#[derive(Debug, Default)]
pub struct X86_64Assembler {
    bytes: Vec<u8>,
    branches: Vec<X86BranchFixup>,
}

impl X86_64Assembler {
    pub fn new() -> Self {
        Self::default()
    }

    /// Encode `MOV destination, source` using the register-direct r64/rm64 form.
    pub fn move_register(&mut self, destination: X86_64Register, source: X86_64Register) {
        if destination == source {
            return;
        }
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

    /// Encode `ADD destination, immediate` when x86 can sign-extend the
    /// immediate without changing the i64 value.
    fn add_immediate(&mut self, destination: X86_64Register, immediate: i64) -> bool {
        self.emit_group1_immediate(destination, 0, immediate)
    }

    /// Encode `SUB destination, immediate` when x86 can sign-extend the
    /// immediate without changing the i64 value.
    fn subtract_immediate(&mut self, destination: X86_64Register, immediate: i64) -> bool {
        self.emit_group1_immediate(destination, 5, immediate)
    }

    /// Encode `AND destination, immediate` when sign extension preserves the
    /// complete i64 mask.
    fn and_immediate(&mut self, destination: X86_64Register, immediate: i64) -> bool {
        self.emit_group1_immediate(destination, 4, immediate)
    }

    /// Encode `XOR destination, immediate` when x86 can sign-extend the
    /// immediate without changing the i64 value.
    fn xor_immediate(&mut self, destination: X86_64Register, immediate: i64) -> bool {
        self.emit_group1_immediate(destination, 6, immediate)
    }

    /// Encode the three-operand signed `IMUL destination, source, immediate`
    /// form. Unlike the register-register form this does not require copying
    /// `source` into `destination` first.
    fn multiply_immediate(
        &mut self,
        destination: X86_64Register,
        source: X86_64Register,
        immediate: i64,
    ) -> bool {
        if let Ok(immediate) = i8::try_from(immediate) {
            self.emit_rex_w(destination, source);
            self.bytes.push(0x6b);
            self.emit_register_modrm(destination, source);
            self.bytes.push(immediate as u8);
            true
        } else if let Ok(immediate) = i32::try_from(immediate) {
            self.emit_rex_w(destination, source);
            self.bytes.push(0x69);
            self.emit_register_modrm(destination, source);
            self.bytes.extend_from_slice(&immediate.to_le_bytes());
            true
        } else {
            false
        }
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

    fn push_register(&mut self, register: X86_64Register) {
        if register.extension() != 0 {
            self.bytes.push(0x41);
        }
        self.bytes.push(0x50 + register.low_bits());
    }

    fn pop_register(&mut self, register: X86_64Register) {
        if register.extension() != 0 {
            self.bytes.push(0x41);
        }
        self.bytes.push(0x58 + register.low_bits());
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
        let encoded = self.compare_immediate(register, i64::from(immediate));
        debug_assert!(encoded);
    }

    fn compare_immediate(&mut self, register: X86_64Register, immediate: i64) -> bool {
        self.emit_group1_immediate(register, 7, immediate)
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
        let instruction = self.bytes.len();
        self.bytes.push(0xe9);
        let displacement = self.bytes.len();
        self.bytes.extend_from_slice(&0i32.to_le_bytes());
        self.branches.push(X86BranchFixup {
            instruction,
            displacement,
            target: None,
            short_opcode: 0xeb,
            near_length: 5,
            relaxable: false,
        });
        displacement
    }

    fn emit_conditional_jump_rel32(&mut self, opcode: u8) -> usize {
        let instruction = self.bytes.len();
        self.bytes.extend_from_slice(&[0x0f, opcode]);
        let displacement = self.bytes.len();
        self.bytes.extend_from_slice(&0i32.to_le_bytes());
        self.branches.push(X86BranchFixup {
            instruction,
            displacement,
            target: None,
            short_opcode: 0x70 | (opcode & 0x0f),
            near_length: 6,
            relaxable: false,
        });
        displacement
    }

    fn patch_rel32(&mut self, displacement: usize, target: usize) {
        let next_instruction = displacement + std::mem::size_of::<i32>();
        let relative = i64::try_from(target).unwrap() - i64::try_from(next_instruction).unwrap();
        let relative = i32::try_from(relative).expect("x86 prototype branch exceeds rel32 range");
        self.bytes[displacement..next_instruction].copy_from_slice(&relative.to_le_bytes());
        self.branches
            .iter_mut()
            .find(|branch| branch.displacement == displacement)
            .expect("x86 rel32 patch does not belong to an emitted branch")
            .target = Some(target);
    }

    fn allow_short_branch(&mut self, displacement: usize) {
        self.branches
            .iter_mut()
            .find(|branch| branch.displacement == displacement)
            .expect("x86 short-branch candidate was not emitted by this assembler")
            .relaxable = true;
    }

    fn align_with_nops(&mut self, code_base_offset: usize, alignment: usize) {
        debug_assert!(alignment.is_power_of_two());
        let address_offset = code_base_offset + self.bytes.len();
        let padding = address_offset.wrapping_neg() & (alignment - 1);
        self.bytes.resize(self.bytes.len() + padding, 0x90);
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

    pub fn finish(mut self) -> Box<[u8]> {
        branch::relax_short_branches(&mut self.bytes, &self.branches);
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

    fn emit_group1_immediate(
        &mut self,
        destination: X86_64Register,
        opcode_extension: u8,
        immediate: i64,
    ) -> bool {
        debug_assert!(opcode_extension < 8);
        let modrm = 0xc0 | (opcode_extension << 3) | destination.low_bits();
        if let Ok(immediate) = i8::try_from(immediate) {
            self.bytes.push(0x48 | destination.extension());
            self.bytes
                .extend_from_slice(&[0x83, modrm, immediate as u8]);
            true
        } else if let Ok(immediate) = i32::try_from(immediate) {
            self.bytes.push(0x48 | destination.extension());
            self.bytes.extend_from_slice(&[0x81, modrm]);
            self.bytes.extend_from_slice(&immediate.to_le_bytes());
            true
        } else {
            false
        }
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
    publication_mask: u64,
    carried_mask: u64,
    required_context_mask: u16,
}

fn required_straight_context_mask(config: &NativeStraightLongLoopConfig) -> u16 {
    config.operations[..config.operation_count as usize]
        .iter()
        .copied()
        .fold(0u16, |mask, operation| {
            let (entry_base, token_count) = match operation {
                NativeStraightLongOperation::HashLoad {
                    entry_base,
                    token_count,
                    ..
                }
                | NativeStraightLongOperation::HashStore {
                    entry_base,
                    token_count,
                    ..
                } => (entry_base, token_count),
                _ => return mask,
            };
            let entries = ((1u32 << token_count) - 1) << entry_base;
            mask | entries as u16
        })
}

fn supports_linear_scalar_residency(config: &NativeStraightLongLoopConfig) -> bool {
    config.operations[..config.operation_count as usize]
        .iter()
        .copied()
        .all(|operation| {
            matches!(
                operation,
                NativeStraightLongOperation::Modulo { .. }
                    | NativeStraightLongOperation::Move { .. }
                    | NativeStraightLongOperation::Binary { .. }
                    | NativeStraightLongOperation::BinaryAssign { .. }
                    | NativeStraightLongOperation::Guard { .. }
            )
        })
}

fn supports_structured_scalar_residency(config: &NativeStraightLongLoopConfig) -> bool {
    config.operations[..config.operation_count as usize]
        .iter()
        .copied()
        .all(|operation| {
            matches!(
                operation,
                NativeStraightLongOperation::Modulo { .. }
                    | NativeStraightLongOperation::Move { .. }
                    | NativeStraightLongOperation::Binary { .. }
                    | NativeStraightLongOperation::BinaryAssign { .. }
                    | NativeStraightLongOperation::Guard { .. }
                    | NativeStraightLongOperation::BranchUnless { .. }
                    | NativeStraightLongOperation::Jump { .. }
            )
        })
}

fn structured_phi_candidate_is_safe(
    config: &NativeStraightLongLoopConfig,
    slot_mask: u64,
    block_starts: &[bool; super::NATIVE_STRAIGHT_LONG_MAX_OPERATIONS + 1],
    definitely_written_before: &[u64; super::NATIVE_STRAIGHT_LONG_MAX_OPERATIONS],
) -> bool {
    config.operations[..config.operation_count as usize]
        .iter()
        .copied()
        .enumerate()
        .all(|(index, operation)| {
            if straight_long_operation_input_mask(operation) & slot_mask == 0
                || definitely_written_before[index] & slot_mask != 0
            {
                return true;
            }
            index != 0
                && !block_starts[index]
                && config.operations[index - 1].output_mask() & slot_mask != 0
        })
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

fn emit_linear_operand_with_resident(
    assembler: &mut X86_64Assembler,
    operand: QuickLongOperand,
    scratch: X86_64Register,
    induction_slot: u16,
    induction_register: X86_64Register,
    resident_values: &[(u64, X86_64Register)],
) -> X86_64Register {
    match operand {
        QuickLongOperand::Slot(slot) if slot == induction_slot => induction_register,
        QuickLongOperand::Slot(slot) => {
            let slot_mask = 1u64 << slot;
            if let Some((_, register)) = resident_values
                .iter()
                .find(|(mask, _)| *mask & slot_mask != 0)
            {
                *register
            } else {
                assembler.move_from_base_disp32(scratch, X86_64Register::RDI, i32::from(slot) * 8);
                scratch
            }
        }
        QuickLongOperand::Const(value) => {
            assembler.move_immediate64(scratch, value);
            scratch
        }
    }
}

fn emit_linear_operand(
    assembler: &mut X86_64Assembler,
    operand: QuickLongOperand,
    destination: X86_64Register,
    induction_slot: u16,
    induction_register: X86_64Register,
    resident_values: &[(u64, X86_64Register)],
) {
    let source = emit_linear_operand_with_resident(
        assembler,
        operand,
        destination,
        induction_slot,
        induction_register,
        resident_values,
    );
    assembler.move_register(destination, source);
}

fn emit_linear_condition_operand(
    assembler: &mut X86_64Assembler,
    operand: NativeStraightLongConditionOperand,
    destination: X86_64Register,
    scratch: X86_64Register,
    induction_slot: u16,
    induction_register: X86_64Register,
    resident_values: &[(u64, X86_64Register)],
) -> X86_64Register {
    match operand {
        NativeStraightLongConditionOperand::Source(source) => emit_linear_operand_with_resident(
            assembler,
            source,
            destination,
            induction_slot,
            induction_register,
            resident_values,
        ),
        NativeStraightLongConditionOperand::BitwiseAnd { lhs, rhs } => {
            let immediate_source = match (lhs, rhs) {
                (source, QuickLongOperand::Const(value)) if i32::try_from(value).is_ok() => {
                    Some((source, value))
                }
                (QuickLongOperand::Const(value), source) if i32::try_from(value).is_ok() => {
                    Some((source, value))
                }
                _ => None,
            };
            if let Some((source, immediate)) = immediate_source {
                let source_register = emit_linear_operand_with_resident(
                    assembler,
                    source,
                    destination,
                    induction_slot,
                    induction_register,
                    resident_values,
                );
                assembler.move_register(destination, source_register);
                let encoded = assembler.and_immediate(destination, immediate);
                debug_assert!(encoded);
                return destination;
            }
            let rhs_register = emit_linear_operand_with_resident(
                assembler,
                rhs,
                scratch,
                induction_slot,
                induction_register,
                resident_values,
            );
            let lhs_register = emit_linear_operand_with_resident(
                assembler,
                lhs,
                destination,
                induction_slot,
                induction_register,
                resident_values,
            );
            assembler.move_register(destination, lhs_register);
            assembler.and_register(destination, rhs_register);
            destination
        }
    }
}

fn emit_linear_condition_compare(
    assembler: &mut X86_64Assembler,
    lhs: NativeStraightLongConditionOperand,
    rhs: NativeStraightLongConditionOperand,
    lhs_destination: X86_64Register,
    rhs_destination: X86_64Register,
    scratch: X86_64Register,
    induction_slot: u16,
    induction_register: X86_64Register,
    resident_values: &[(u64, X86_64Register)],
) {
    if let NativeStraightLongConditionOperand::Source(QuickLongOperand::Const(immediate)) = rhs
        && i32::try_from(immediate).is_ok()
    {
        let lhs_register = emit_linear_condition_operand(
            assembler,
            lhs,
            lhs_destination,
            scratch,
            induction_slot,
            induction_register,
            resident_values,
        );
        let encoded = assembler.compare_immediate(lhs_register, immediate);
        debug_assert!(encoded);
        return;
    }

    let lhs_register = emit_linear_condition_operand(
        assembler,
        lhs,
        lhs_destination,
        scratch,
        induction_slot,
        induction_register,
        resident_values,
    );
    let rhs_register = emit_linear_condition_operand(
        assembler,
        rhs,
        rhs_destination,
        scratch,
        induction_slot,
        induction_register,
        resident_values,
    );
    assembler.compare_register(lhs_register, rhs_register);
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

fn emit_signed_power_of_two_remainder(
    assembler: &mut X86_64Assembler,
    result: X86_64Register,
    mask_scratch: X86_64Register,
    sign_scratch: X86_64Register,
    mask: i64,
) {
    // PHP's signed remainder truncates division toward zero. For 2^k:
    // bias = sign(value) & mask; ((value + bias) & mask) - bias.
    assembler.move_register(sign_scratch, result);
    assembler.arithmetic_shift_right_immediate8(sign_scratch, 63);
    if assembler.and_immediate(sign_scratch, mask) {
        assembler.add_register(result, sign_scratch);
        let encoded = assembler.and_immediate(result, mask);
        debug_assert!(encoded);
    } else {
        assembler.move_immediate64(mask_scratch, mask);
        assembler.and_register(sign_scratch, mask_scratch);
        assembler.add_register(result, sign_scratch);
        assembler.and_register(result, mask_scratch);
    }
    assembler.subtract_register(result, sign_scratch);
}

fn emit_x86_straight_return(
    assembler: &mut X86_64Assembler,
    uses_context: bool,
    saved_resident_registers: &[X86_64Register],
) {
    for register in saved_resident_registers.iter().copied().rev() {
        assembler.pop_register(register);
    }
    if uses_context {
        assembler.pop_register(X86_64Register::R12);
    }
    assembler.return_near();
}

fn emit_x86_resident_publications(
    assembler: &mut X86_64Assembler,
    resident_values: &[(u64, X86_64Register)],
) {
    for (mut slot_mask, register) in resident_values.iter().copied() {
        while slot_mask != 0 {
            let slot = slot_mask.trailing_zeros() as u16;
            slot_mask &= slot_mask - 1;
            assembler.move_to_base_disp32(X86_64Register::RDI, register, i32::from(slot) * 8);
        }
    }
}

fn emit_x86_resident_output(
    assembler: &mut X86_64Assembler,
    output_mask: u64,
    source: X86_64Register,
    resident_values: &[(u64, X86_64Register)],
) {
    for (slot_mask, register) in resident_values.iter().copied() {
        if slot_mask & output_mask != 0 && register != source {
            assembler.move_register(register, source);
        }
    }
}

fn x86_embedded_loop_bound(bound: QuickLongOperand) -> Option<i64> {
    match bound {
        QuickLongOperand::Const(value) if i32::try_from(value).is_ok() => Some(value),
        QuickLongOperand::Slot(_) | QuickLongOperand::Const(_) => None,
    }
}

fn emit_x86_loop_bound_compare(
    assembler: &mut X86_64Assembler,
    induction: X86_64Register,
    bound: X86_64Register,
    embedded_bound: Option<i64>,
) {
    if let Some(embedded_bound) = embedded_bound {
        let encoded = assembler.compare_immediate(induction, embedded_bound);
        debug_assert!(encoded);
    } else {
        assembler.compare_register(induction, bound);
    }
}

fn x86_direct_resident_result_register(
    operation: NativeStraightLongOperation,
    resident_values: &[(u64, X86_64Register)],
) -> Option<X86_64Register> {
    let can_write_directly = match operation {
        NativeStraightLongOperation::Move { .. }
        | NativeStraightLongOperation::Binary {
            kind:
                ScalarLongOpKind::Add
                | ScalarLongOpKind::Subtract
                | ScalarLongOpKind::Multiply
                | ScalarLongOpKind::BitwiseXor,
            ..
        }
        | NativeStraightLongOperation::BinaryAssign {
            kind:
                ScalarLongOpKind::Add
                | ScalarLongOpKind::Subtract
                | ScalarLongOpKind::Multiply
                | ScalarLongOpKind::BitwiseXor,
            ..
        } => true,
        NativeStraightLongOperation::Modulo { divisor, .. } => {
            signed_power_of_two_remainder_mask(divisor).is_some()
        }
        NativeStraightLongOperation::Binary {
            kind: ScalarLongOpKind::Modulo,
            rhs: QuickLongOperand::Const(divisor),
            ..
        }
        | NativeStraightLongOperation::BinaryAssign {
            kind: ScalarLongOpKind::Modulo,
            rhs: QuickLongOperand::Const(divisor),
            ..
        } => signed_power_of_two_remainder_mask(divisor).is_some(),
        _ => false,
    };
    if !can_write_directly {
        return None;
    }

    let output_mask = operation.output_mask();
    resident_values
        .iter()
        .copied()
        .find_map(|(slot_mask, register)| (slot_mask & output_mask != 0).then_some(register))
}

#[allow(clippy::too_many_arguments)]
fn emit_token_immediate_select(
    assembler: &mut X86_64Assembler,
    source: u16,
    values: &[i64; 4],
    token_count: u8,
    token: X86_64Register,
    result: X86_64Register,
    induction_slot: u16,
    induction: X86_64Register,
    resident_values: &[(u64, X86_64Register)],
    operation_index: u8,
    operation_side_exit_jumps: &mut Vec<(usize, u8)>,
) {
    emit_linear_operand(
        assembler,
        QuickLongOperand::Slot(source),
        token,
        induction_slot,
        induction,
        resident_values,
    );
    let mut selected_jumps = Vec::with_capacity(token_count as usize);
    for index in 0..token_count as usize {
        assembler.compare_immediate8(token, index as i8);
        let next = assembler.jump_not_equal_rel32();
        assembler.move_immediate64(result, values[index]);
        selected_jumps.push(assembler.jump_rel32());
        let next_offset = assembler.bytes.len();
        assembler.patch_rel32(next, next_offset);
    }
    operation_side_exit_jumps.push((assembler.jump_rel32(), operation_index));
    let selected = assembler.bytes.len();
    for jump in selected_jumps {
        assembler.patch_rel32(jump, selected);
    }
}

#[allow(clippy::too_many_arguments)]
fn emit_context_entry_select(
    assembler: &mut X86_64Assembler,
    key: u16,
    entry_base: u8,
    token_count: u8,
    token: X86_64Register,
    destination: X86_64Register,
    context: X86_64Register,
    induction_slot: u16,
    induction: X86_64Register,
    resident_values: &[(u64, X86_64Register)],
    operation_index: u8,
    operation_side_exit_jumps: &mut Vec<(usize, u8)>,
) {
    emit_linear_operand(
        assembler,
        QuickLongOperand::Slot(key),
        token,
        induction_slot,
        induction,
        resident_values,
    );
    let mut selected_jumps = Vec::with_capacity(token_count as usize);
    for token_index in 0..token_count as usize {
        assembler.compare_immediate8(token, token_index as i8);
        let next = assembler.jump_not_equal_rel32();
        assembler.move_register(destination, context);
        let entry_index = usize::from(entry_base) + token_index;
        assembler.move_from_base_disp32(
            destination,
            destination,
            i32::try_from(entry_index * std::mem::size_of::<*mut i64>()).unwrap(),
        );
        selected_jumps.push(assembler.jump_rel32());
        let next_offset = assembler.bytes.len();
        assembler.patch_rel32(next, next_offset);
    }
    operation_side_exit_jumps.push((assembler.jump_rel32(), operation_index));
    let selected = assembler.bytes.len();
    for jump in selected_jumps {
        assembler.patch_rel32(jump, selected);
    }
}

fn emit_scalar_straight_loop(
    config: &NativeStraightLongLoopConfig,
    checked: bool,
    budgeted: bool,
    polling_interval: Option<u16>,
    publication_mask: u64,
    carried_mask: u64,
    defer_visible_phi: bool,
    code_base_offset: usize,
) -> Result<Box<[u8]>, X86StraightLongLoopError> {
    debug_assert!(!(budgeted && polling_interval.is_some()));
    let mut assembler = X86_64Assembler::new();
    let slots = X86_64Register::RDI;
    // Keep the loop induction outside RAX/RDX so signed division can use its
    // architectural dividend and remainder pair without spilling loop state.
    let induction = X86_64Register::R11;
    let bound = X86_64Register::RCX;
    let embeddable_bound = x86_embedded_loop_bound(config.bound);
    let lhs = X86_64Register::RAX;
    let rhs = X86_64Register::R8;
    let auxiliary = X86_64Register::R9;
    let polling_remaining = X86_64Register::R10;
    let context = X86_64Register::R12;
    let uses_context = required_straight_context_mask(config) != 0;
    let keeps_linear_scalar_values_resident =
        polling_interval.is_some() && supports_linear_scalar_residency(config);
    let keeps_structured_scalar_values_resident = polling_interval.is_some()
        && !keeps_linear_scalar_values_resident
        && supports_structured_scalar_residency(config);
    let keeps_structured_carried_values_resident =
        keeps_structured_scalar_values_resident && carried_mask != 0;
    let keeps_carried_values_resident = (keeps_linear_scalar_values_resident
        || keeps_structured_carried_values_resident)
        && carried_mask != 0;
    if carried_mask != 0 && !keeps_carried_values_resident {
        return Err(X86StraightLongLoopError::UnsupportedConfig(
            "x86 carried state requires a range-proven scalar loop",
        ));
    }
    if carried_mask & !publication_mask != 0
        || carried_mask.count_ones() > 3
        || config
            .post_result
            .is_some_and(|slot| carried_mask & (1u64 << slot) != 0)
    {
        return Err(X86StraightLongLoopError::UnsupportedConfig(
            "x86 carried state exceeds fixed publication registers",
        ));
    }
    let structured_block_starts = if keeps_structured_scalar_values_resident {
        straight_long_structured_block_starts(config)
    } else {
        [false; super::NATIVE_STRAIGHT_LONG_MAX_OPERATIONS + 1]
    };
    let (structured_definitely_written_before, structured_definitely_written_exit) =
        if keeps_structured_scalar_values_resident {
            straight_long_structured_definitely_written(config)
        } else {
            ([0; super::NATIVE_STRAIGHT_LONG_MAX_OPERATIONS], 0)
        };
    let mut carried_values = [
        (0u64, X86_64Register::R13),
        (0u64, X86_64Register::R14),
        (0u64, X86_64Register::R15),
        (0u64, X86_64Register::RCX),
    ];
    let mut remaining_carried = carried_mask;
    for resident in &mut carried_values {
        if remaining_carried == 0 {
            break;
        }
        let slot_mask = 1u64 << remaining_carried.trailing_zeros();
        remaining_carried &= remaining_carried - 1;
        resident.0 = slot_mask;
    }
    let carried_count = carried_mask.count_ones() as usize;
    let mut resident_values = carried_values;
    let mut deferred_publication_values = carried_values;
    let mut resident_initial_load_masks = [
        carried_values[0].0,
        carried_values[1].0,
        carried_values[2].0,
        0,
    ];
    let mut structured_definition_operations_by_register = [0u64; 4];
    let resident_capacity = if embeddable_bound.is_some() { 4 } else { 3 };
    let mut next_resident = carried_count;
    if keeps_structured_scalar_values_resident && defer_visible_phi {
        let mut phi_candidates =
            publication_mask & structured_definitely_written_exit & !carried_mask;
        while phi_candidates != 0 {
            let slot_mask = 1u64 << phi_candidates.trailing_zeros();
            phi_candidates &= phi_candidates - 1;
            if !structured_phi_candidate_is_safe(
                config,
                slot_mask,
                &structured_block_starts,
                &structured_definitely_written_before,
            ) {
                continue;
            }
            let definition_operations = config.operations[..config.operation_count as usize]
                .iter()
                .copied()
                .enumerate()
                .fold(0u64, |mask, (index, operation)| {
                    if operation.output_mask() & slot_mask != 0 {
                        mask | (1u64 << index)
                    } else {
                        mask
                    }
                });
            debug_assert_ne!(definition_operations, 0);
            let resident_index = if let Some(index) =
                (carried_count..next_resident).find(|&index| {
                    structured_definition_operations_by_register[index] == definition_operations
                }) {
                index
            } else if next_resident < resident_capacity {
                let index = next_resident;
                next_resident += 1;
                structured_definition_operations_by_register[index] = definition_operations;
                index
            } else {
                continue;
            };
            resident_values[resident_index].0 |= slot_mask;
            deferred_publication_values[resident_index].0 |= slot_mask;
        }
    }
    let deferred_publication_mask = deferred_publication_values
        .iter()
        .fold(0u64, |mask, (slot_mask, _)| mask | slot_mask);
    let caches_scalar_invariants = config.operations[..config.operation_count as usize]
        .iter()
        .copied()
        .all(|operation| {
            matches!(
                operation,
                NativeStraightLongOperation::Modulo { .. }
                    | NativeStraightLongOperation::Move { .. }
                    | NativeStraightLongOperation::Binary { .. }
                    | NativeStraightLongOperation::BinaryAssign { .. }
                    | NativeStraightLongOperation::BranchUnless { .. }
                    | NativeStraightLongOperation::Jump { .. }
            )
        });
    let invariant_slot_masks = if caches_scalar_invariants {
        straight_long_best_invariant_slot_masks(config)
    } else {
        [0; 2]
    };
    for invariant_slot_mask in invariant_slot_masks {
        if invariant_slot_mask == 0 || next_resident >= resident_capacity {
            continue;
        }
        resident_values[next_resident].0 = invariant_slot_mask;
        resident_initial_load_masks[next_resident] = invariant_slot_mask;
        next_resident += 1;
    }
    // An immediate compare is one byte longer than CMP r64,r64 for imm8
    // bounds (and four bytes longer for imm32). Only pay that loop-body cost
    // when RCX actually carries a fourth resident value; otherwise preserve
    // the dedicated bound register and the shorter backedge.
    let embedded_bound =
        (resident_values[3].0 != 0).then(|| embeddable_bound.expect("resident RCX needs a bound"));
    let mut saved_resident_registers = Vec::with_capacity(3);
    let displacement = |slot: u16| i32::from(slot) * 8;

    if uses_context {
        assembler.push_register(context);
        let incoming_context = if budgeted || polling_interval.is_some() {
            X86_64Register::RDX
        } else {
            X86_64Register::RSI
        };
        assembler.move_register(context, incoming_context);
    }
    for (index, (slot_mask, register)) in resident_values.iter().copied().enumerate() {
        if slot_mask == 0 {
            continue;
        }
        if register != X86_64Register::RCX {
            assembler.push_register(register);
            saved_resident_registers.push(register);
        }
        let initial_load_mask = resident_initial_load_masks[index];
        if initial_load_mask != 0 {
            assembler.move_from_base_disp32(
                register,
                slots,
                displacement(initial_load_mask.trailing_zeros() as u16),
            );
        }
    }

    assembler.move_from_base_disp32(induction, slots, displacement(config.induction_slot));
    if embedded_bound.is_none() {
        emit_linear_operand(
            &mut assembler,
            config.bound,
            bound,
            config.induction_slot,
            induction,
            &resident_values,
        );
    }
    if let Some(interval) = polling_interval {
        assembler.move_immediate64(polling_remaining, i64::from(interval));
    }
    emit_x86_loop_bound_compare(&mut assembler, induction, bound, embedded_bound);
    let completed_jump = assembler.jump_greater_or_equal_rel32();
    if polling_interval.is_some() && keeps_structured_scalar_values_resident {
        assembler.align_with_nops(code_base_offset, X86_STRUCTURED_LOOP_ALIGNMENT);
    }
    let loop_start = assembler.bytes.len();
    let mut operation_side_exit_jumps = Vec::new();
    let mut structured_conditional_jumps = Vec::new();
    let mut structured_jumps = Vec::new();
    let mut operation_offsets = [0usize; super::NATIVE_STRAIGHT_LONG_MAX_OPERATIONS + 1];
    let linear_live_after = straight_long_linear_live_after(config);
    let structured_local_resident_output_masks = if keeps_structured_scalar_values_resident {
        straight_long_structured_local_resident_output_masks(
            config,
            publication_mask,
            carried_mask,
            &structured_block_starts,
        )
    } else {
        [0; super::NATIVE_STRAIGHT_LONG_MAX_OPERATIONS]
    };
    let mut latest_output_mask = 0u64;

    for (operation_index, operation) in config.operations[..config.operation_count as usize]
        .iter()
        .copied()
        .enumerate()
    {
        operation_offsets[operation_index] = assembler.bytes.len();
        if keeps_structured_scalar_values_resident && structured_block_starts[operation_index] {
            latest_output_mask = 0;
        }
        let mut active_fixed_resident_values = resident_values;
        if keeps_structured_scalar_values_resident {
            for resident_index in 0..active_fixed_resident_values.len() {
                let publication_slots = deferred_publication_values[resident_index].0;
                if structured_definition_operations_by_register[resident_index] != 0
                    && structured_definitely_written_before[operation_index] & publication_slots
                        != publication_slots
                {
                    active_fixed_resident_values[resident_index].0 = 0;
                }
            }
        }
        let active_resident_values = [
            active_fixed_resident_values[0],
            active_fixed_resident_values[1],
            active_fixed_resident_values[2],
            active_fixed_resident_values[3],
            (latest_output_mask, X86_64Register::RDX),
        ];
        let shadow_store_mask = if keeps_linear_scalar_values_resident {
            straight_long_linear_shadow_store_mask(
                config,
                operation_index,
                publication_mask,
                &linear_live_after,
            ) & !deferred_publication_mask
        } else if keeps_structured_scalar_values_resident {
            operation.shadow_output_mask()
                & !deferred_publication_mask
                & !structured_local_resident_output_masks[operation_index]
        } else {
            operation.shadow_output_mask() & !deferred_publication_mask
        };
        let direct_result_register = (!checked && polling_interval.is_some())
            .then(|| x86_direct_resident_result_register(operation, &deferred_publication_values))
            .flatten();
        let direct_result_needs_local_forwarding = direct_result_register.is_some()
            && operation_index + 1 < config.operation_count as usize
            && (!keeps_structured_scalar_values_resident
                || !structured_block_starts[operation_index + 1])
            && straight_long_operation_input_mask(config.operations[operation_index + 1])
                & operation.output_mask()
                & !deferred_publication_mask
                != 0;
        let (kind, left, right, result, destination) = match operation {
            NativeStraightLongOperation::Move { source, result } => {
                let result_scratch = direct_result_register.unwrap_or(lhs);
                let source_register = emit_linear_operand_with_resident(
                    &mut assembler,
                    source,
                    result_scratch,
                    config.induction_slot,
                    induction,
                    &active_resident_values,
                );
                let result_register = if let Some(direct_result_register) = direct_result_register {
                    assembler.move_register(direct_result_register, source_register);
                    direct_result_register
                } else {
                    source_register
                };
                emit_x86_resident_output(
                    &mut assembler,
                    1u64 << result,
                    result_register,
                    &deferred_publication_values,
                );
                if keeps_linear_scalar_values_resident || keeps_structured_scalar_values_resident {
                    if direct_result_register.is_none() || direct_result_needs_local_forwarding {
                        assembler.move_register(X86_64Register::RDX, result_register);
                        latest_output_mask = 1u64 << result;
                    } else {
                        latest_output_mask = 0;
                    }
                }
                if shadow_store_mask & (1u64 << result) != 0 {
                    assembler.move_to_base_disp32(slots, result_register, displacement(result));
                }
                continue;
            }
            NativeStraightLongOperation::StringToken { token, result } => {
                assembler.move_immediate64(lhs, i64::from(token));
                assembler.move_to_base_disp32(slots, lhs, displacement(result));
                continue;
            }
            NativeStraightLongOperation::StringLength {
                source,
                lengths,
                token_count,
                result,
            } => {
                emit_token_immediate_select(
                    &mut assembler,
                    source,
                    &lengths,
                    token_count,
                    lhs,
                    rhs,
                    config.induction_slot,
                    induction,
                    &resident_values,
                    operation_index as u8,
                    &mut operation_side_exit_jumps,
                );
                assembler.move_to_base_disp32(slots, rhs, displacement(result));
                continue;
            }
            NativeStraightLongOperation::HashLoad {
                key,
                entry_base,
                token_count,
                result,
                destination,
            } => {
                emit_context_entry_select(
                    &mut assembler,
                    key,
                    entry_base,
                    token_count,
                    lhs,
                    auxiliary,
                    context,
                    config.induction_slot,
                    induction,
                    &resident_values,
                    operation_index as u8,
                    &mut operation_side_exit_jumps,
                );
                assembler.move_from_base_disp32(rhs, auxiliary, 0);
                assembler.move_to_base_disp32(slots, rhs, displacement(result));
                if let Some(destination) = destination
                    && destination != result
                {
                    assembler.move_to_base_disp32(slots, rhs, displacement(destination));
                }
                continue;
            }
            NativeStraightLongOperation::HashStore {
                key,
                entry_base,
                token_count,
                source,
            } => {
                emit_context_entry_select(
                    &mut assembler,
                    key,
                    entry_base,
                    token_count,
                    lhs,
                    auxiliary,
                    context,
                    config.induction_slot,
                    induction,
                    &resident_values,
                    operation_index as u8,
                    &mut operation_side_exit_jumps,
                );
                emit_linear_operand(
                    &mut assembler,
                    source,
                    rhs,
                    config.induction_slot,
                    induction,
                    &resident_values,
                );
                assembler.move_to_base_disp32(auxiliary, rhs, 0);
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
                emit_linear_condition_compare(
                    &mut assembler,
                    condition_lhs,
                    condition_rhs,
                    lhs,
                    rhs,
                    auxiliary,
                    config.induction_slot,
                    induction,
                    &resident_values,
                );
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
                if false_target as usize == operation_index + 1 {
                    // Both outcomes enter the same physical successor. The
                    // predicate has no materialized PHP result in this IR.
                    continue;
                }
                emit_linear_condition_compare(
                    &mut assembler,
                    condition_lhs,
                    condition_rhs,
                    lhs,
                    rhs,
                    auxiliary,
                    config.induction_slot,
                    induction,
                    &active_resident_values,
                );
                let branch = emit_false_condition_jump(&mut assembler, kind);
                assembler.allow_short_branch(branch);
                structured_conditional_jumps.push((branch, false_target));
                continue;
            }
            NativeStraightLongOperation::Jump { target } => {
                if target as usize == operation_index + 1 {
                    continue;
                }
                let branch = assembler.jump_rel32();
                assembler.allow_short_branch(branch);
                structured_jumps.push((branch, target));
                continue;
            }
            _ => {
                return Err(X86StraightLongLoopError::UnsupportedConfig(
                    "x86 scalar loop operation is not lowered",
                ));
            }
        };
        let result_register = direct_result_register.unwrap_or(lhs);
        let embedded_immediate = match (kind, right) {
            (
                ScalarLongOpKind::Add
                | ScalarLongOpKind::Subtract
                | ScalarLongOpKind::Multiply
                | ScalarLongOpKind::BitwiseXor,
                QuickLongOperand::Const(value),
            ) if i32::try_from(value).is_ok() => Some(value),
            _ => None,
        };
        let remainder_mask = match (kind, right) {
            (ScalarLongOpKind::Modulo, QuickLongOperand::Const(divisor)) => {
                signed_power_of_two_remainder_mask(divisor)
            }
            _ => None,
        };
        let mut right_register =
            (embedded_immediate.is_none() && remainder_mask.is_none()).then(|| {
                emit_linear_operand_with_resident(
                    &mut assembler,
                    right,
                    rhs,
                    config.induction_slot,
                    induction,
                    &active_resident_values,
                )
            });
        if right_register == Some(result_register) && result_register != lhs {
            // x86 arithmetic overwrites its left operand. Preserve an old
            // right-side value before reusing its fixed register as the
            // result destination.
            assembler.move_register(rhs, result_register);
            right_register = Some(rhs);
        } else if result_register != lhs
            && matches!(
                right_register,
                Some(X86_64Register::R13 | X86_64Register::R14 | X86_64Register::R15)
            )
        {
            // On Zen-family cores, two long-lived publication operands can
            // repeatedly collide in the banked integer register file. A
            // short-lived copy re-banks the right operand and is measurably
            // faster for dependent fixed-to-fixed recurrences.
            assembler.move_register(rhs, right_register.unwrap());
            right_register = Some(rhs);
        }
        let left_register = emit_linear_operand_with_resident(
            &mut assembler,
            left,
            result_register,
            config.induction_slot,
            induction,
            &active_resident_values,
        );
        if kind != ScalarLongOpKind::Multiply || embedded_immediate.is_none() {
            assembler.move_register(result_register, left_register);
        }
        if matches!(kind, ScalarLongOpKind::IntDivide | ScalarLongOpKind::Modulo)
            && remainder_mask.is_none()
            && right_register == Some(X86_64Register::RDX)
        {
            // CQO owns RDX, so a resident divisor must leave the architectural
            // dividend pair before sign extension.
            assembler.move_register(rhs, X86_64Register::RDX);
            right_register = Some(rhs);
        }
        match kind {
            ScalarLongOpKind::Add => match embedded_immediate {
                Some(immediate) => {
                    let encoded = assembler.add_immediate(result_register, immediate);
                    debug_assert!(encoded);
                }
                None => assembler.add_register(result_register, right_register.unwrap()),
            },
            ScalarLongOpKind::Subtract => {
                if let Some(immediate) = embedded_immediate {
                    let encoded = assembler.subtract_immediate(result_register, immediate);
                    debug_assert!(encoded);
                } else {
                    assembler.subtract_register(result_register, right_register.unwrap());
                }
            }
            ScalarLongOpKind::Multiply => {
                if let Some(immediate) = embedded_immediate {
                    let encoded =
                        assembler.multiply_immediate(result_register, left_register, immediate);
                    debug_assert!(encoded);
                } else {
                    assembler.multiply_register(result_register, right_register.unwrap());
                }
            }
            ScalarLongOpKind::BitwiseXor => match embedded_immediate {
                Some(immediate) => {
                    let encoded = assembler.xor_immediate(result_register, immediate);
                    debug_assert!(encoded);
                }
                None => assembler.xor_register(result_register, right_register.unwrap()),
            },
            ScalarLongOpKind::Modulo if remainder_mask.is_some() => {
                emit_signed_power_of_two_remainder(
                    &mut assembler,
                    result_register,
                    rhs,
                    auxiliary,
                    remainder_mask.unwrap(),
                );
            }
            ScalarLongOpKind::IntDivide | ScalarLongOpKind::Modulo => {
                let right_register = right_register.unwrap();
                debug_assert_eq!(result_register, lhs);
                if checked {
                    assembler.compare_immediate8(right_register, 0);
                    operation_side_exit_jumps
                        .push((assembler.jump_equal_rel32(), operation_index as u8));
                    assembler.compare_immediate8(right_register, -1);
                    let safe_divisor = assembler.jump_not_equal_rel32();
                    assembler.move_immediate64(auxiliary, i64::MIN);
                    assembler.compare_register(lhs, auxiliary);
                    operation_side_exit_jumps
                        .push((assembler.jump_equal_rel32(), operation_index as u8));
                    let divide = assembler.bytes.len();
                    assembler.patch_rel32(safe_divisor, divide);
                }
                assembler.sign_extend_rax_into_rdx();
                assembler.signed_divide(right_register);
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
        emit_x86_resident_output(
            &mut assembler,
            operation.output_mask(),
            result_register,
            &deferred_publication_values,
        );
        if keeps_linear_scalar_values_resident || keeps_structured_scalar_values_resident {
            if direct_result_register.is_none() || direct_result_needs_local_forwarding {
                assembler.move_register(X86_64Register::RDX, result_register);
                latest_output_mask = operation.output_mask();
            } else {
                latest_output_mask = 0;
            }
        }
        if shadow_store_mask & (1u64 << result) != 0 {
            assembler.move_to_base_disp32(slots, result_register, displacement(result));
        }
        if let Some(destination) = destination
            && destination != result
            && shadow_store_mask & (1u64 << destination) != 0
        {
            assembler.move_to_base_disp32(slots, result_register, displacement(destination));
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
    emit_x86_loop_bound_compare(&mut assembler, induction, bound, embedded_bound);
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
        emit_x86_resident_publications(&mut assembler, &deferred_publication_values);
        assembler.move_immediate32_eax(X86_STRAIGHT_CHUNK_EXHAUSTED);
        emit_x86_straight_return(&mut assembler, uses_context, &saved_resident_registers);
    }

    let completed = assembler.bytes.len();
    if let Some(completed_after_iteration_jump) = completed_after_iteration_jump {
        assembler.patch_rel32(completed_after_iteration_jump, completed);
    }
    for loop_jump in loop_jumps {
        assembler.patch_rel32(loop_jump, loop_start);
    }
    assembler.move_to_base_disp32(slots, induction, displacement(config.induction_slot));
    emit_x86_resident_publications(&mut assembler, &deferred_publication_values);
    assembler.clear_eax();
    emit_x86_straight_return(&mut assembler, uses_context, &saved_resident_registers);

    let empty_completed = assembler.bytes.len();
    assembler.patch_rel32(completed_jump, empty_completed);
    assembler.move_to_base_disp32(slots, induction, displacement(config.induction_slot));
    assembler.clear_eax();
    emit_x86_straight_return(&mut assembler, uses_context, &saved_resident_registers);

    operation_side_exit_jumps.sort_unstable_by_key(|(_, operation)| *operation);
    let unique_side_exit_count = operation_side_exit_jumps
        .iter()
        .map(|(_, operation)| *operation)
        .fold((0usize, None), |(count, previous), operation| {
            (
                count + usize::from(previous != Some(operation)),
                Some(operation),
            )
        })
        .0;
    if unique_side_exit_count == 1 {
        let side_exit = assembler.bytes.len();
        let operation_index = operation_side_exit_jumps[0].1;
        for (jump, operation) in operation_side_exit_jumps {
            debug_assert_eq!(operation, operation_index);
            assembler.patch_rel32(jump, side_exit);
        }
        assembler.move_to_base_disp32(slots, induction, displacement(config.induction_slot));
        emit_x86_resident_publications(&mut assembler, &deferred_publication_values);
        let status = X86_STRAIGHT_OPERATION_SIDE_EXIT | (u32::from(operation_index) << 8);
        assembler.move_immediate32_eax(status);
        emit_x86_straight_return(&mut assembler, uses_context, &saved_resident_registers);
    } else if unique_side_exit_count > 1 {
        let mut common_epilogue_jumps = Vec::with_capacity(unique_side_exit_count);
        let mut cursor = 0;
        while cursor < operation_side_exit_jumps.len() {
            let operation_index = operation_side_exit_jumps[cursor].1;
            let selector = assembler.bytes.len();
            while cursor < operation_side_exit_jumps.len()
                && operation_side_exit_jumps[cursor].1 == operation_index
            {
                assembler.patch_rel32(operation_side_exit_jumps[cursor].0, selector);
                cursor += 1;
            }
            let status = X86_STRAIGHT_OPERATION_SIDE_EXIT | (u32::from(operation_index) << 8);
            assembler.move_immediate32_eax(status);
            common_epilogue_jumps.push(assembler.jump_rel32());
        }
        let common_epilogue = assembler.bytes.len();
        for jump in common_epilogue_jumps {
            assembler.patch_rel32(jump, common_epilogue);
        }
        assembler.move_to_base_disp32(slots, induction, displacement(config.induction_slot));
        emit_x86_resident_publications(&mut assembler, &deferred_publication_values);
        emit_x86_straight_return(&mut assembler, uses_context, &saved_resident_registers);
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
            NativeStraightLongOperation::StringToken { token, result } => {
                if token >= 4 {
                    return Err(X86StraightLongLoopError::UnsupportedConfig(
                        "x86 finite String token exceeds the shared token table",
                    ));
                }
                validate_output(result)?;
                written_mask |= 1u64 << result;
            }
            NativeStraightLongOperation::StringLength {
                source,
                token_count,
                result,
                ..
            } => {
                validate_slot(source)?;
                if token_count == 0 || token_count > 4 {
                    return Err(X86StraightLongLoopError::UnsupportedConfig(
                        "x86 String token count is outside the finite token table",
                    ));
                }
                validate_output(result)?;
                written_mask |= 1u64 << result;
            }
            NativeStraightLongOperation::HashLoad {
                key,
                entry_base,
                token_count,
                result,
                destination,
            } => {
                validate_slot(key)?;
                if token_count == 0
                    || token_count > 4
                    || usize::from(entry_base) + usize::from(token_count)
                        > super::NATIVE_STRAIGHT_LONG_MAX_CONTEXT_ENTRIES
                {
                    return Err(X86StraightLongLoopError::UnsupportedConfig(
                        "x86 hash context selection is outside the shared entry table",
                    ));
                }
                validate_output(result)?;
                written_mask |= 1u64 << result;
                if let Some(destination) = destination {
                    validate_output(destination)?;
                    written_mask |= 1u64 << destination;
                }
            }
            NativeStraightLongOperation::HashStore {
                key,
                entry_base,
                token_count,
                source,
            } => {
                validate_slot(key)?;
                validate_operand(source)?;
                if token_count == 0
                    || token_count > 4
                    || usize::from(entry_base) + usize::from(token_count)
                        > super::NATIVE_STRAIGHT_LONG_MAX_CONTEXT_ENTRIES
                {
                    return Err(X86StraightLongLoopError::UnsupportedConfig(
                        "x86 hash context selection is outside the shared entry table",
                    ));
                }
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
        Self::compile_with_metadata(config, u64::MAX, 0, false)
    }

    pub(super) fn compile_range_proven_polling_with_publication_and_carried(
        config: NativeStraightLongLoopConfig,
        publication_mask: u64,
        carried_mask: u64,
    ) -> Result<Self, X86StraightLongLoopError> {
        if carried_mask & !publication_mask != 0 || carried_mask.count_ones() > 3 {
            return Err(X86StraightLongLoopError::UnsupportedConfig(
                "x86 carried state exceeds fixed publication registers",
            ));
        }
        Self::compile_with_metadata(config, publication_mask, carried_mask, true)
    }

    fn compile_with_metadata(
        config: NativeStraightLongLoopConfig,
        publication_mask: u64,
        carried_mask: u64,
        defer_visible_phi: bool,
    ) -> Result<Self, X86StraightLongLoopError> {
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
            return Self::compile_scalar(config, publication_mask, carried_mask, defer_visible_phi);
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
            publication_mask,
            carried_mask,
            required_context_mask: 0,
        })
    }

    fn compile_scalar(
        config: NativeStraightLongLoopConfig,
        publication_mask: u64,
        carried_mask: u64,
        defer_visible_phi: bool,
    ) -> Result<Self, X86StraightLongLoopError> {
        validate_scalar_straight_config(&config)?;
        let polling_carried_mask = ((supports_linear_scalar_residency(&config)
            || supports_structured_scalar_residency(&config))
            && !config
                .post_result
                .is_some_and(|slot| carried_mask & (1u64 << slot) != 0))
        .then_some(carried_mask)
        .unwrap_or(0);
        let fast_code =
            emit_scalar_straight_loop(&config, false, false, None, u64::MAX, 0, false, 0)?;
        let checked_entry_offset = fast_code.len();
        let checked_code =
            emit_scalar_straight_loop(&config, true, false, None, u64::MAX, 0, false, 0)?;
        let mut code = fast_code.into_vec();
        code.extend_from_slice(&checked_code);
        let chunk_entry_offset = code.len();
        let chunk_code =
            emit_scalar_straight_loop(&config, false, true, None, u64::MAX, 0, false, 0)?;
        code.extend_from_slice(&chunk_code);
        let checked_chunk_entry_offset = code.len();
        let checked_chunk_code =
            emit_scalar_straight_loop(&config, true, true, None, u64::MAX, 0, false, 0)?;
        code.extend_from_slice(&checked_chunk_code);
        let polling_entry_offset = code.len();
        let polling_code = emit_scalar_straight_loop(
            &config,
            false,
            false,
            Some(X86_STRAIGHT_SAFEPOINT_INTERVAL),
            publication_mask,
            polling_carried_mask,
            defer_visible_phi,
            polling_entry_offset,
        )?;
        code.extend_from_slice(&polling_code);
        let code = code.into_boxed_slice();
        let memory = ExecutableMemory::from_code(&code)?;
        let required_context_mask = required_straight_context_mask(&config);
        Ok(Self {
            memory,
            code,
            config,
            checked_entry_offset,
            chunk_entry_offset,
            checked_chunk_entry_offset,
            polling_entry_offset,
            publication_mask,
            carried_mask,
            required_context_mask,
        })
    }

    pub fn call(
        &self,
        slots: &mut [i64; 64],
    ) -> Result<NativeStraightLongLoopResult, X86StraightLongLoopError> {
        if self.required_context_mask != 0 {
            return Err(X86StraightLongLoopError::UnsupportedConfig(
                "x86 straight-loop hash operation requires runtime context",
            ));
        }
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
        if self.required_context_mask != 0 {
            return Err(X86StraightLongLoopError::UnsupportedConfig(
                "x86 straight-loop hash operation requires runtime context",
            ));
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

    pub fn call_chunk_with_context(
        &self,
        slots: &mut [i64; 64],
        iteration_budget: u64,
        entry_pointers: &[*mut i64; super::NATIVE_STRAIGHT_LONG_MAX_CONTEXT_ENTRIES],
    ) -> Result<NativeStraightLongLoopResult, X86StraightLongLoopError> {
        if iteration_budget == 0 {
            return Err(X86StraightLongLoopError::ZeroIterationBudget);
        }
        let mut required = self.required_context_mask;
        while required != 0 {
            let index = required.trailing_zeros() as usize;
            required &= required - 1;
            if entry_pointers[index].is_null() {
                return Err(X86StraightLongLoopError::UnsupportedConfig(
                    "x86 straight-loop hash context contains a null entry pointer",
                ));
            }
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
        let entry = unsafe { self.memory.entry().add(entry_offset) };
        type NativeFunction = unsafe extern "C" fn(*mut i64, u64, *const *mut i64) -> u32;
        let function: NativeFunction = unsafe { std::mem::transmute(entry) };
        let status = unsafe {
            function(
                slots.as_mut_ptr(),
                iteration_budget,
                entry_pointers.as_ptr(),
            )
        };
        self.decode_status(status)
    }

    fn call_proven_polling(
        &self,
        slots: &mut [i64; 64],
        interrupt_flag: *const bool,
    ) -> Result<NativeStraightLongLoopResult, X86StraightLongLoopError> {
        debug_assert_eq!(self.required_context_mask, 0);
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

    fn publication_mask(&self) -> u64 {
        self.publication_mask
    }

    fn carried_mask(&self) -> u64 {
        self.carried_mask
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
    straight_range_proven_polling_compiled: OnceCell<Option<CompiledX86StraightLongLoop>>,
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
            straight_range_proven_polling_compiled: OnceCell::new(),
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
        safepoint_interval: u16,
        publication_mask: u64,
        carried_mask: u64,
    ) -> Option<&CompiledX86StraightLongLoop> {
        if safepoint_interval != X86_STRAIGHT_SAFEPOINT_INTERVAL {
            return None;
        }
        let program = self
            .straight_range_proven_polling_compiled
            .get_or_init(|| {
                CompiledX86StraightLongLoop::compile_range_proven_polling_with_publication_and_carried(
                    *config,
                    publication_mask,
                    carried_mask,
                )
                .ok()
            })
            .as_ref()?;
        (program.config() == *config
            && program.publication_mask() == publication_mask
            && program.carried_mask() == carried_mask)
            .then_some(program)
    }

    pub fn prepare_call_program(
        &self,
        _target_identities: [usize; super::NATIVE_QUICK_LONG_MAX_CALL_TARGETS],
        _target_count: u8,
        config: NativeStraightLongLoopConfig,
    ) -> Option<&CompiledX86StraightLongLoop> {
        // The builder revalidates the guarded target tree on every region
        // entry. The compiled config comparison below additionally prevents a
        // semantically different scalar plan from reusing this mapping.
        self.prepare_straight_program(&config)
    }

    pub fn dispatch_prepared_call_chunk(
        &self,
        program: &CompiledX86StraightLongLoop,
        slots: &mut [i64; 64],
        iteration_budget: u64,
    ) -> Result<NativeStraightLongLoopResult, X86StraightLongLoopError> {
        self.dispatch_prepared_straight_chunk(program, slots, iteration_budget)
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

    pub fn dispatch_prepared_straight_chunk_with_context(
        &self,
        program: &CompiledX86StraightLongLoop,
        slots: &mut [i64; 64],
        iteration_budget: u64,
        entry_pointers: &[*mut i64; super::NATIVE_STRAIGHT_LONG_MAX_CONTEXT_ENTRIES],
    ) -> Result<NativeStraightLongLoopResult, X86StraightLongLoopError> {
        self.native_calls
            .set(self.native_calls.get().saturating_add(1));
        self.native_chunks
            .set(self.native_chunks.get().saturating_add(1));
        let outcome = program.call_chunk_with_context(slots, iteration_budget, entry_pointers);
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
            || matches!(
                self.straight_range_proven_polling_compiled.get(),
                Some(Some(_))
            )
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

const X86_SCALAR_STATUS_SUCCESS: u32 = 0;
const X86_SCALAR_STATUS_SIDE_EXIT: u32 = 1;
const MAX_SCALAR_LONG_INPUTS: usize = 8;
const MAX_SCALAR_LONG_OPERATIONS: usize = 8;

pub const SCALAR_LONG_JIT_HOT_THRESHOLD: u16 = 64;

#[derive(Debug)]
pub enum ScalarLongJitError {
    InvalidProgram(&'static str),
    InputCount { expected: usize, actual: usize },
    InvalidNativeStatus(u32),
    Memory(io::Error),
}

impl fmt::Display for ScalarLongJitError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::InvalidProgram(message) => formatter.write_str(message),
            Self::InputCount { expected, actual } => {
                write!(
                    formatter,
                    "expected {expected} scalar inputs, received {actual}"
                )
            }
            Self::InvalidNativeStatus(status) => {
                write!(formatter, "x86 scalar JIT returned unknown status {status}")
            }
            Self::Memory(error) => write!(formatter, "executable memory error: {error}"),
        }
    }
}

impl std::error::Error for ScalarLongJitError {
    fn source(&self) -> Option<&(dyn std::error::Error + 'static)> {
        match self {
            Self::Memory(error) => Some(error),
            _ => None,
        }
    }
}

impl From<io::Error> for ScalarLongJitError {
    fn from(error: io::Error) -> Self {
        Self::Memory(error)
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ScalarLongJitOutcome {
    Value(i64),
    SideExit,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ScalarLongJitDispatch {
    Interpret,
    Value(i64),
    SideExit,
}

/// Lazy per-plan standalone scalar cache. The same target-neutral plan and
/// hotness contract are used by the ARM64 backend.
pub struct ScalarLongJitCache {
    calls: Cell<u16>,
    compiled: OnceCell<Option<CompiledScalarLongProgram>>,
    native_entries: Cell<u64>,
    side_exits: Cell<u64>,
}

impl ScalarLongJitCache {
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
        plan: &ScalarLongFunctionPlan,
        arguments: &[i64; MAX_SCALAR_LONG_INPUTS],
    ) -> ScalarLongJitDispatch {
        if plan.select.is_none() && plan.program.operations.len() < 2 {
            return ScalarLongJitDispatch::Interpret;
        }

        if self.compiled.get().is_none() {
            let calls = self.calls.get().saturating_add(1);
            self.calls.set(calls);
            if calls < SCALAR_LONG_JIT_HOT_THRESHOLD {
                return ScalarLongJitDispatch::Interpret;
            }
            let _ = self
                .compiled
                .set(CompiledScalarLongProgram::compile(plan).ok());
        }

        let Some(program) = self.compiled.get().and_then(Option::as_ref) else {
            return ScalarLongJitDispatch::Interpret;
        };
        self.native_entries
            .set(self.native_entries.get().saturating_add(1));
        match program.call(&arguments[..plan.public_args as usize]) {
            Ok(ScalarLongJitOutcome::Value(value)) => ScalarLongJitDispatch::Value(value),
            Ok(ScalarLongJitOutcome::SideExit) | Err(_) => {
                self.side_exits.set(self.side_exits.get().saturating_add(1));
                ScalarLongJitDispatch::SideExit
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

impl Default for ScalarLongJitCache {
    fn default() -> Self {
        Self::new()
    }
}

/// Memory-backed lowering of a typed scalar function or method leaf.
///
/// SysV ABI: RDI points to inputs, RSI to the output word and RDX to eight
/// private temporary words. EAX returns zero on success or one for an exact
/// checked-arithmetic side exit. The output word is written only on success.
pub struct CompiledScalarLongProgram {
    memory: ExecutableMemory,
    code: Box<[u8]>,
    input_count: usize,
}

impl CompiledScalarLongProgram {
    pub fn compile(plan: &ScalarLongFunctionPlan) -> Result<Self, ScalarLongJitError> {
        validate_x86_scalar_long_plan(plan)?;
        let mut assembler = X86_64Assembler::new();
        let temporaries = X86_64Register::R10;
        assembler.move_register(temporaries, X86_64Register::RDX);
        let mut side_exit_jumps = Vec::new();
        let mut selected_true_join = None;

        if let Some(select) = plan.select {
            let shared_end = select.shared_operation_count as usize;
            let true_end = shared_end + select.when_true_operation_count as usize;
            emit_x86_scalar_operations(
                &mut assembler,
                &plan.program.operations,
                0,
                shared_end,
                &mut side_exit_jumps,
            );
            emit_x86_scalar_condition_compare(&mut assembler, select.lhs, select.rhs);
            let selected_false = emit_false_condition_jump(&mut assembler, select.kind);

            emit_x86_scalar_operations(
                &mut assembler,
                &plan.program.operations,
                shared_end,
                true_end,
                &mut side_exit_jumps,
            );
            emit_x86_scalar_output(&mut assembler, select.when_true);
            selected_true_join = Some(assembler.jump_rel32());

            let false_offset = assembler.bytes.len();
            assembler.patch_rel32(selected_false, false_offset);
            emit_x86_scalar_operations(
                &mut assembler,
                &plan.program.operations,
                true_end,
                plan.program.operations.len(),
                &mut side_exit_jumps,
            );
            emit_x86_scalar_output(&mut assembler, select.when_false);
        } else {
            emit_x86_scalar_operations(
                &mut assembler,
                &plan.program.operations,
                0,
                plan.program.operations.len(),
                &mut side_exit_jumps,
            );
            emit_x86_scalar_output(&mut assembler, plan.program.outputs[0]);
        }

        let success = assembler.bytes.len();
        if let Some(join) = selected_true_join {
            assembler.patch_rel32(join, success);
        }
        assembler.move_immediate32_eax(X86_SCALAR_STATUS_SUCCESS);
        assembler.return_near();

        let side_exit = assembler.bytes.len();
        assembler.move_immediate32_eax(X86_SCALAR_STATUS_SIDE_EXIT);
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

    pub fn call(&self, inputs: &[i64]) -> Result<ScalarLongJitOutcome, ScalarLongJitError> {
        if inputs.len() != self.input_count {
            return Err(ScalarLongJitError::InputCount {
                expected: self.input_count,
                actual: inputs.len(),
            });
        }
        type NativeFunction = unsafe extern "C" fn(*const i64, *mut i64, *mut i64) -> u32;
        let function: NativeFunction = unsafe { std::mem::transmute(self.memory.entry()) };
        let mut output = MaybeUninit::<i64>::uninit();
        let mut temporaries = [0_i64; MAX_SCALAR_LONG_OPERATIONS];
        let status = unsafe {
            function(
                inputs.as_ptr(),
                output.as_mut_ptr(),
                temporaries.as_mut_ptr(),
            )
        };
        match status {
            X86_SCALAR_STATUS_SUCCESS => {
                Ok(ScalarLongJitOutcome::Value(unsafe { output.assume_init() }))
            }
            X86_SCALAR_STATUS_SIDE_EXIT => Ok(ScalarLongJitOutcome::SideExit),
            status => Err(ScalarLongJitError::InvalidNativeStatus(status)),
        }
    }

    pub fn code(&self) -> &[u8] {
        &self.code
    }
}

fn emit_x86_scalar_operations(
    assembler: &mut X86_64Assembler,
    operations: &[ScalarLongOp],
    start: usize,
    end: usize,
    side_exit_jumps: &mut Vec<usize>,
) {
    for (relative_index, operation) in operations[start..end].iter().copied().enumerate() {
        emit_x86_scalar_operation(
            assembler,
            start + relative_index,
            operation,
            side_exit_jumps,
        );
    }
}

fn emit_x86_scalar_operation(
    assembler: &mut X86_64Assembler,
    index: usize,
    operation: ScalarLongOp,
    side_exit_jumps: &mut Vec<usize>,
) {
    let lhs = X86_64Register::RAX;
    let rhs = X86_64Register::R8;
    let auxiliary = X86_64Register::R9;
    let embedded_immediate = match (operation.kind, operation.rhs) {
        (
            ScalarLongOpKind::Add
            | ScalarLongOpKind::Subtract
            | ScalarLongOpKind::Multiply
            | ScalarLongOpKind::BitwiseXor,
            ScalarLongSource::Constant(value),
        ) if i32::try_from(value).is_ok() => Some(value),
        _ => None,
    };
    let remainder_mask = match (operation.kind, operation.rhs) {
        (ScalarLongOpKind::Modulo, ScalarLongSource::Constant(divisor)) => {
            signed_power_of_two_remainder_mask(divisor)
        }
        _ => None,
    };
    emit_x86_scalar_source(assembler, operation.lhs, lhs);
    if embedded_immediate.is_none() && remainder_mask.is_none() {
        emit_x86_scalar_source(assembler, operation.rhs, rhs);
    }

    match operation.kind {
        ScalarLongOpKind::Add => {
            if let Some(immediate) = embedded_immediate {
                let encoded = assembler.add_immediate(lhs, immediate);
                debug_assert!(encoded);
            } else {
                assembler.add_register(lhs, rhs);
            }
            side_exit_jumps.push(assembler.jump_overflow_rel32());
        }
        ScalarLongOpKind::Subtract => {
            if let Some(immediate) = embedded_immediate {
                let encoded = assembler.subtract_immediate(lhs, immediate);
                debug_assert!(encoded);
            } else {
                assembler.subtract_register(lhs, rhs);
            }
            side_exit_jumps.push(assembler.jump_overflow_rel32());
        }
        ScalarLongOpKind::Multiply => {
            if let Some(immediate) = embedded_immediate {
                let encoded = assembler.multiply_immediate(lhs, lhs, immediate);
                debug_assert!(encoded);
            } else {
                assembler.multiply_register(lhs, rhs);
            }
            side_exit_jumps.push(assembler.jump_overflow_rel32());
        }
        ScalarLongOpKind::BitwiseXor => {
            if let Some(immediate) = embedded_immediate {
                let encoded = assembler.xor_immediate(lhs, immediate);
                debug_assert!(encoded);
            } else {
                assembler.xor_register(lhs, rhs);
            }
        }
        ScalarLongOpKind::Modulo if remainder_mask.is_some() => {
            emit_signed_power_of_two_remainder(
                assembler,
                lhs,
                rhs,
                auxiliary,
                remainder_mask.unwrap(),
            );
        }
        ScalarLongOpKind::IntDivide | ScalarLongOpKind::Modulo => {
            emit_x86_scalar_division_guards(assembler, lhs, rhs, auxiliary, side_exit_jumps);
            assembler.sign_extend_rax_into_rdx();
            assembler.signed_divide(rhs);
            if operation.kind == ScalarLongOpKind::Modulo {
                assembler.move_register(lhs, X86_64Register::RDX);
            }
        }
    }
    assembler.move_to_base_disp32(X86_64Register::R10, lhs, i32::try_from(index * 8).unwrap());
}

fn emit_x86_scalar_division_guards(
    assembler: &mut X86_64Assembler,
    lhs: X86_64Register,
    rhs: X86_64Register,
    auxiliary: X86_64Register,
    side_exit_jumps: &mut Vec<usize>,
) {
    assembler.compare_immediate8(rhs, 0);
    side_exit_jumps.push(assembler.jump_equal_rel32());
    assembler.compare_immediate8(rhs, -1);
    let safe_divisor = assembler.jump_not_equal_rel32();
    assembler.move_immediate64(auxiliary, i64::MIN);
    assembler.compare_register(lhs, auxiliary);
    side_exit_jumps.push(assembler.jump_equal_rel32());
    let divide = assembler.bytes.len();
    assembler.patch_rel32(safe_divisor, divide);
}

fn emit_x86_scalar_source(
    assembler: &mut X86_64Assembler,
    source: ScalarLongSource,
    destination: X86_64Register,
) {
    match source {
        ScalarLongSource::Input(index) => {
            assembler.move_from_base_disp32(destination, X86_64Register::RDI, i32::from(index) * 8)
        }
        ScalarLongSource::Constant(value) => assembler.move_immediate64(destination, value),
        ScalarLongSource::Temporary(index) => {
            assembler.move_from_base_disp32(destination, X86_64Register::R10, i32::from(index) * 8)
        }
    }
}

fn emit_x86_scalar_condition_operand(
    assembler: &mut X86_64Assembler,
    operand: ScalarLongConditionOperand,
    destination: X86_64Register,
    scratch: X86_64Register,
) {
    match operand {
        ScalarLongConditionOperand::Source(source) => {
            emit_x86_scalar_source(assembler, source, destination);
        }
        ScalarLongConditionOperand::BitwiseAnd { lhs, rhs } => {
            let immediate_source = match (lhs, rhs) {
                (source, ScalarLongSource::Constant(value)) if i32::try_from(value).is_ok() => {
                    Some((source, value))
                }
                (ScalarLongSource::Constant(value), source) if i32::try_from(value).is_ok() => {
                    Some((source, value))
                }
                _ => None,
            };
            if let Some((source, immediate)) = immediate_source {
                emit_x86_scalar_source(assembler, source, destination);
                let encoded = assembler.and_immediate(destination, immediate);
                debug_assert!(encoded);
                return;
            }
            emit_x86_scalar_source(assembler, lhs, destination);
            emit_x86_scalar_source(assembler, rhs, scratch);
            assembler.and_register(destination, scratch);
        }
    }
}

fn emit_x86_scalar_condition_compare(
    assembler: &mut X86_64Assembler,
    lhs: ScalarLongConditionOperand,
    rhs: ScalarLongConditionOperand,
) {
    if let ScalarLongConditionOperand::Source(ScalarLongSource::Constant(immediate)) = rhs
        && i32::try_from(immediate).is_ok()
    {
        emit_x86_scalar_condition_operand(assembler, lhs, X86_64Register::RAX, X86_64Register::R9);
        let encoded = assembler.compare_immediate(X86_64Register::RAX, immediate);
        debug_assert!(encoded);
        return;
    }

    emit_x86_scalar_condition_operand(assembler, lhs, X86_64Register::RAX, X86_64Register::R9);
    emit_x86_scalar_condition_operand(assembler, rhs, X86_64Register::R8, X86_64Register::R9);
    assembler.compare_register(X86_64Register::RAX, X86_64Register::R8);
}

fn emit_x86_scalar_output(assembler: &mut X86_64Assembler, source: ScalarLongSource) {
    emit_x86_scalar_source(assembler, source, X86_64Register::RAX);
    assembler.move_to_base_disp32(X86_64Register::RSI, X86_64Register::RAX, 0);
}

fn validate_x86_scalar_long_plan(plan: &ScalarLongFunctionPlan) -> Result<(), ScalarLongJitError> {
    if plan.public_args as usize > MAX_SCALAR_LONG_INPUTS {
        return Err(ScalarLongJitError::InvalidProgram(
            "too many public inputs for the x86 scalar ABI",
        ));
    }
    if plan.program.operations.len() > MAX_SCALAR_LONG_OPERATIONS {
        return Err(ScalarLongJitError::InvalidProgram(
            "too many operations for the x86 scalar temporary ABI",
        ));
    }
    if plan.program.output_count != 1 {
        return Err(ScalarLongJitError::InvalidProgram(
            "the scalar leaf must expose exactly one output",
        ));
    }

    if let Some(select) = plan.select {
        validate_x86_scalar_select(plan, select)
    } else {
        for (index, operation) in plan.program.operations.iter().enumerate() {
            validate_x86_scalar_source(operation.lhs, index, plan.public_args)?;
            validate_x86_scalar_source(operation.rhs, index, plan.public_args)?;
        }
        validate_x86_scalar_source(
            plan.program.outputs[0],
            plan.program.operations.len(),
            plan.public_args,
        )
    }
}

fn validate_x86_scalar_select(
    plan: &ScalarLongFunctionPlan,
    select: crate::vm::function::ScalarLongSelect,
) -> Result<(), ScalarLongJitError> {
    let operation_count = plan.program.operations.len();
    let shared_end = select.shared_operation_count as usize;
    let true_end = shared_end
        .checked_add(select.when_true_operation_count as usize)
        .ok_or(ScalarLongJitError::InvalidProgram(
            "conditional operation ranges overflow",
        ))?;
    if shared_end > operation_count || true_end > operation_count {
        return Err(ScalarLongJitError::InvalidProgram(
            "conditional operation range is outside the program",
        ));
    }
    for (index, operation) in plan.program.operations[..shared_end].iter().enumerate() {
        validate_x86_scalar_source(operation.lhs, index, plan.public_args)?;
        validate_x86_scalar_source(operation.rhs, index, plan.public_args)?;
    }
    validate_x86_scalar_condition_operand(select.lhs, shared_end, plan.public_args)?;
    validate_x86_scalar_condition_operand(select.rhs, shared_end, plan.public_args)?;

    for (index, operation) in plan.program.operations[shared_end..true_end]
        .iter()
        .enumerate()
    {
        let absolute_index = shared_end + index;
        validate_x86_scalar_source(operation.lhs, absolute_index, plan.public_args)?;
        validate_x86_scalar_source(operation.rhs, absolute_index, plan.public_args)?;
    }
    validate_x86_scalar_source(select.when_true, true_end, plan.public_args)?;

    for (index, operation) in plan.program.operations[true_end..].iter().enumerate() {
        let absolute_index = true_end + index;
        validate_x86_false_edge_source(
            operation.lhs,
            shared_end,
            true_end,
            absolute_index,
            plan.public_args,
        )?;
        validate_x86_false_edge_source(
            operation.rhs,
            shared_end,
            true_end,
            absolute_index,
            plan.public_args,
        )?;
    }
    validate_x86_false_edge_source(
        select.when_false,
        shared_end,
        true_end,
        operation_count,
        plan.public_args,
    )
}

fn validate_x86_scalar_condition_operand(
    operand: ScalarLongConditionOperand,
    available_temporaries: usize,
    input_count: u8,
) -> Result<(), ScalarLongJitError> {
    match operand {
        ScalarLongConditionOperand::Source(source) => {
            validate_x86_scalar_source(source, available_temporaries, input_count)
        }
        ScalarLongConditionOperand::BitwiseAnd { lhs, rhs } => {
            validate_x86_scalar_source(lhs, available_temporaries, input_count)?;
            validate_x86_scalar_source(rhs, available_temporaries, input_count)
        }
    }
}

fn validate_x86_false_edge_source(
    source: ScalarLongSource,
    shared_end: usize,
    false_start: usize,
    available_temporaries: usize,
    input_count: u8,
) -> Result<(), ScalarLongJitError> {
    match source {
        ScalarLongSource::Temporary(index)
            if !((index as usize) < shared_end
                || ((index as usize) >= false_start
                    && (index as usize) < available_temporaries)) =>
        {
            Err(ScalarLongJitError::InvalidProgram(
                "false edge references a true-edge temporary",
            ))
        }
        _ => validate_x86_scalar_source(source, available_temporaries, input_count),
    }
}

fn validate_x86_scalar_source(
    source: ScalarLongSource,
    available_temporaries: usize,
    input_count: u8,
) -> Result<(), ScalarLongJitError> {
    match source {
        ScalarLongSource::Input(index) if index >= u16::from(input_count) => Err(
            ScalarLongJitError::InvalidProgram("input index is outside the public ABI"),
        ),
        ScalarLongSource::Temporary(index) if index as usize >= available_temporaries => Err(
            ScalarLongJitError::InvalidProgram("temporary is used before it is defined"),
        ),
        _ => Ok(()),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn invariant_operands_are_loaded_once_per_native_entry() {
        let mut operations = [NativeStraightLongOperation::Unused;
            super::super::NATIVE_STRAIGHT_LONG_MAX_OPERATIONS];
        operations[0] = NativeStraightLongOperation::BranchUnless {
            kind: ScalarLongConditionKind::LessThan,
            lhs: NativeStraightLongConditionOperand::Source(QuickLongOperand::Slot(0)),
            rhs: NativeStraightLongConditionOperand::Source(QuickLongOperand::Slot(3)),
            false_target: 3,
        };
        operations[1] = NativeStraightLongOperation::BinaryAssign {
            kind: ScalarLongOpKind::Add,
            lhs: QuickLongOperand::Slot(0),
            rhs: QuickLongOperand::Slot(3),
            result: 6,
            destination: 1,
        };
        operations[2] = NativeStraightLongOperation::Jump { target: 4 };
        operations[3] = NativeStraightLongOperation::BinaryAssign {
            kind: ScalarLongOpKind::Subtract,
            lhs: QuickLongOperand::Slot(0),
            rhs: QuickLongOperand::Const(1),
            result: 7,
            destination: 1,
        };
        operations[4] = NativeStraightLongOperation::BinaryAssign {
            kind: ScalarLongOpKind::Add,
            lhs: QuickLongOperand::Slot(1),
            rhs: QuickLongOperand::Slot(4),
            result: 8,
            destination: 2,
        };
        let program = CompiledX86StraightLongLoop::compile(NativeStraightLongLoopConfig {
            induction_slot: 0,
            bound: QuickLongOperand::Const(100),
            operations,
            operation_count: 5,
            post_result: None,
        })
        .unwrap();

        let slot_3_load = [0x4c, 0x8b, 0xaf, 0x18, 0x00, 0x00, 0x00];
        let slot_4_load = [0x4c, 0x8b, 0xb7, 0x20, 0x00, 0x00, 0x00];
        assert_eq!(
            program
                .code()
                .windows(slot_3_load.len())
                .filter(|window| *window == slot_3_load)
                .count(),
            5,
            "each of the five ABI entries should load invariant slot 3 once"
        );
        assert_eq!(
            program
                .code()
                .windows(slot_4_load.len())
                .filter(|window| *window == slot_4_load)
                .count(),
            5,
            "each of the five ABI entries should load invariant slot 4 once"
        );
        let dedicated_bound_load = [0x48, 0xb9, 0x64, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00];
        assert_eq!(
            program
                .code()
                .windows(dedicated_bound_load.len())
                .filter(|window| *window == dedicated_bound_load)
                .count(),
            5,
            "constant bounds should stay in RCX unless a fourth resident value uses it"
        );

        let mut slots = [0i64; 64];
        slots[3] = 50;
        slots[4] = 7;
        let outcome = program.call_chunk(&mut slots, 128).unwrap();
        assert_eq!(outcome.outcome, NativeStraightLongLoopOutcome::Completed);
        assert_eq!(
            (slots[0], slots[1], slots[2], slots[3], slots[4]),
            (100, 98, 105, 50, 7)
        );
    }

    #[test]
    fn finite_string_hash_context_survives_signed_division_abi_registers() {
        let mut operations = [NativeStraightLongOperation::Unused;
            super::super::NATIVE_STRAIGHT_LONG_MAX_OPERATIONS];
        operations[0] = NativeStraightLongOperation::StringToken {
            token: 1,
            result: 2,
        };
        operations[1] = NativeStraightLongOperation::StringLength {
            source: 2,
            lengths: [4, 5, 0, 0],
            token_count: 2,
            result: 3,
        };
        operations[2] = NativeStraightLongOperation::HashLoad {
            key: 2,
            entry_base: 0,
            token_count: 2,
            result: 4,
            destination: None,
        };
        operations[3] = NativeStraightLongOperation::Binary {
            kind: ScalarLongOpKind::IntDivide,
            lhs: QuickLongOperand::Slot(4),
            rhs: QuickLongOperand::Slot(3),
            result: 5,
        };
        operations[4] = NativeStraightLongOperation::HashStore {
            key: 2,
            entry_base: 0,
            token_count: 2,
            source: QuickLongOperand::Slot(5),
        };
        let program = CompiledX86StraightLongLoop::compile(NativeStraightLongLoopConfig {
            induction_slot: 0,
            bound: QuickLongOperand::Slot(1),
            operations,
            operation_count: 5,
            post_result: None,
        })
        .expect("finite String and contextual hash operations should lower on x86");
        assert!(
            !program
                .code()
                .windows(2)
                .any(|window| window == [0x41, 0x55]),
            "mixed context entries must not pay the scalar invariant R13 prologue"
        );

        let mut left = 7i64;
        let mut right = 20i64;
        let mut entries =
            [std::ptr::null_mut(); super::super::NATIVE_STRAIGHT_LONG_MAX_CONTEXT_ENTRIES];
        entries[0] = &mut left;
        entries[1] = &mut right;
        let mut slots = [0i64; 64];
        slots[1] = 1;
        let outcome = program
            .call_chunk_with_context(&mut slots, 8, &entries)
            .unwrap();

        assert_eq!(outcome.outcome, NativeStraightLongLoopOutcome::Completed);
        assert_eq!(slots[0], 1);
        assert_eq!(slots[2], 1);
        assert_eq!(slots[3], 5);
        assert_eq!(slots[4], 20);
        assert_eq!(slots[5], 4);
        assert_eq!(left, 7);
        assert_eq!(right, 4);
    }

    #[test]
    fn invalid_finite_string_token_side_exits_before_hash_mutation() {
        let mut operations = [NativeStraightLongOperation::Unused;
            super::super::NATIVE_STRAIGHT_LONG_MAX_OPERATIONS];
        operations[0] = NativeStraightLongOperation::StringLength {
            source: 2,
            lengths: [4, 5, 0, 0],
            token_count: 2,
            result: 3,
        };
        operations[1] = NativeStraightLongOperation::HashStore {
            key: 2,
            entry_base: 0,
            token_count: 2,
            source: QuickLongOperand::Const(99),
        };
        let program = CompiledX86StraightLongLoop::compile(NativeStraightLongLoopConfig {
            induction_slot: 0,
            bound: QuickLongOperand::Slot(1),
            operations,
            operation_count: 2,
            post_result: None,
        })
        .expect("guarded finite String operations should lower on x86");

        let mut left = 7i64;
        let mut right = 20i64;
        let mut entries =
            [std::ptr::null_mut(); super::super::NATIVE_STRAIGHT_LONG_MAX_CONTEXT_ENTRIES];
        entries[0] = &mut left;
        entries[1] = &mut right;
        let mut slots = [0i64; 64];
        slots[1] = 1;
        slots[2] = 3;
        slots[3] = -1;
        let outcome = program
            .call_chunk_with_context(&mut slots, 8, &entries)
            .unwrap();

        assert_eq!(
            outcome.outcome,
            NativeStraightLongLoopOutcome::OperationSideExit
        );
        assert_eq!(outcome.failed_operation, Some(0));
        assert_eq!(slots[0], 0);
        assert_eq!(slots[3], -1);
        assert_eq!(left, 7);
        assert_eq!(right, 20);
    }
    use crate::jit::NATIVE_STRAIGHT_LONG_MAX_OPERATIONS;
    use crate::vm::function::{ScalarLongProgram, ScalarLongSelect};

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

    fn scalar_plan(
        public_args: u8,
        operations: Vec<ScalarLongOp>,
        output: ScalarLongSource,
    ) -> ScalarLongFunctionPlan {
        ScalarLongFunctionPlan::new(
            public_args,
            ScalarLongProgram {
                operations: operations.into_boxed_slice(),
                outputs: [output],
                output_count: 1,
            },
            None,
        )
    }

    fn conditional_scalar_plan(
        public_args: u8,
        operations: Vec<ScalarLongOp>,
        select: ScalarLongSelect,
    ) -> ScalarLongFunctionPlan {
        ScalarLongFunctionPlan::new(
            public_args,
            ScalarLongProgram {
                operations: operations.into_boxed_slice(),
                outputs: [select.when_true],
                output_count: 1,
            },
            Some(select),
        )
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
    fn standalone_scalar_program_executes_and_side_exits_on_overflow() {
        let plan = scalar_plan(
            3,
            vec![
                ScalarLongOp {
                    kind: ScalarLongOpKind::Add,
                    lhs: ScalarLongSource::Input(0),
                    rhs: ScalarLongSource::Input(1),
                },
                ScalarLongOp {
                    kind: ScalarLongOpKind::Multiply,
                    lhs: ScalarLongSource::Temporary(0),
                    rhs: ScalarLongSource::Input(2),
                },
            ],
            ScalarLongSource::Temporary(1),
        );
        let program = CompiledScalarLongProgram::compile(&plan).unwrap();
        assert_eq!(
            program.call(&[7, 5, 3]).unwrap(),
            ScalarLongJitOutcome::Value(36)
        );
        assert_eq!(
            program.call(&[i64::MAX, 1, 3]).unwrap(),
            ScalarLongJitOutcome::SideExit
        );
    }

    #[test]
    fn standalone_scalar_lowering_embeds_imm32_multiply_and_keeps_overflow_exit() {
        let plan = scalar_plan(
            1,
            vec![ScalarLongOp {
                kind: ScalarLongOpKind::Multiply,
                lhs: ScalarLongSource::Input(0),
                rhs: ScalarLongSource::Constant(129),
            }],
            ScalarLongSource::Temporary(0),
        );
        let program = CompiledScalarLongProgram::compile(&plan).unwrap();
        let imul_imm32 = [0x48, 0x69, 0xc0, 0x81, 0x00, 0x00, 0x00];
        assert!(
            program
                .code()
                .windows(imul_imm32.len())
                .any(|window| window == imul_imm32),
            "constant multiply should lower directly to IMUL r64, r64, imm32"
        );
        assert_eq!(
            program.call(&[-7]).unwrap(),
            ScalarLongJitOutcome::Value(-903)
        );
        assert_eq!(
            program.call(&[i64::MAX]).unwrap(),
            ScalarLongJitOutcome::SideExit
        );
    }

    #[test]
    fn standalone_conditional_scalar_program_executes_only_selected_edge() {
        let plan = conditional_scalar_plan(
            1,
            vec![
                ScalarLongOp {
                    kind: ScalarLongOpKind::Multiply,
                    lhs: ScalarLongSource::Input(0),
                    rhs: ScalarLongSource::Constant(3),
                },
                ScalarLongOp {
                    kind: ScalarLongOpKind::Multiply,
                    lhs: ScalarLongSource::Input(0),
                    rhs: ScalarLongSource::Constant(5),
                },
            ],
            ScalarLongSelect {
                kind: ScalarLongConditionKind::Equal,
                lhs: ScalarLongConditionOperand::BitwiseAnd {
                    lhs: ScalarLongSource::Input(0),
                    rhs: ScalarLongSource::Constant(1),
                },
                rhs: ScalarLongConditionOperand::Source(ScalarLongSource::Constant(0)),
                shared_operation_count: 0,
                when_true_operation_count: 1,
                when_true: ScalarLongSource::Temporary(0),
                when_false: ScalarLongSource::Temporary(1),
            },
        );
        let program = CompiledScalarLongProgram::compile(&plan).unwrap();
        assert_eq!(program.call(&[4]).unwrap(), ScalarLongJitOutcome::Value(12));
        assert_eq!(program.call(&[5]).unwrap(), ScalarLongJitOutcome::Value(25));
        assert!(
            program
                .code()
                .windows(4)
                .any(|window| window == [0x48, 0x83, 0xe0, 0x01]),
            "bitwise condition should encode AND RAX, 1"
        );
        assert!(
            program
                .code()
                .windows(4)
                .any(|window| window == [0x48, 0x83, 0xf8, 0x00]),
            "condition should encode CMP RAX, 0"
        );
        assert!(
            !program
                .code()
                .windows(2)
                .any(|window| window == [0x49, 0xb8]),
            "constant condition rhs should not materialize in R8"
        );
    }

    #[test]
    fn standalone_scalar_cache_compiles_at_shared_hotness_threshold() {
        let plan = scalar_plan(
            2,
            vec![
                ScalarLongOp {
                    kind: ScalarLongOpKind::Add,
                    lhs: ScalarLongSource::Input(0),
                    rhs: ScalarLongSource::Input(1),
                },
                ScalarLongOp {
                    kind: ScalarLongOpKind::Multiply,
                    lhs: ScalarLongSource::Temporary(0),
                    rhs: ScalarLongSource::Constant(3),
                },
            ],
            ScalarLongSource::Temporary(1),
        );
        let mut arguments = [0_i64; MAX_SCALAR_LONG_INPUTS];
        arguments[0] = 7;
        arguments[1] = 5;
        for _ in 1..SCALAR_LONG_JIT_HOT_THRESHOLD {
            assert_eq!(
                plan.native_jit().dispatch(&plan, &arguments),
                ScalarLongJitDispatch::Interpret
            );
        }
        assert_eq!(
            plan.native_jit().dispatch(&plan, &arguments),
            ScalarLongJitDispatch::Value(36)
        );
        assert!(plan.native_jit().is_compiled());
        assert_eq!(plan.native_jit().native_entries(), 1);
    }

    #[test]
    fn encoder_sets_rex_extensions_for_high_registers() {
        let mut assembler = X86_64Assembler::new();
        assembler.move_register(X86_64Register::R8, X86_64Register::R9);
        assert_eq!(&*assembler.finish(), &[0x4d, 0x8b, 0xc1]);
    }

    #[test]
    fn encoder_relaxes_forward_branches_and_repatches_remaining_rel32() {
        let mut assembler = X86_64Assembler::new();
        let first = assembler.jump_not_equal_rel32();
        assembler.allow_short_branch(first);
        assembler.bytes.resize(124, 0x90);
        let second = assembler.jump_rel32();
        assembler.allow_short_branch(second);
        assembler.bytes.resize(134, 0x90);
        assembler.patch_rel32(first, 134);
        assembler.patch_rel32(second, 134);
        let backward = assembler.jump_rel32();
        assembler.patch_rel32(backward, 0);
        let far = assembler.jump_equal_rel32();
        assembler.allow_short_branch(far);
        assembler.bytes.resize(273, 0x90);
        assembler.patch_rel32(far, 273);

        let code = assembler.finish();
        assert_eq!(code.len(), 266);
        assert_eq!(&code[..2], &[0x75, 0x7d]);
        assert_eq!(&code[120..122], &[0xeb, 0x05]);
        assert_eq!(&code[127..132], &[0xe9, 0x7c, 0xff, 0xff, 0xff]);
        assert_eq!(&code[132..138], &[0x0f, 0x84, 0x80, 0x00, 0x00, 0x00]);
    }

    #[test]
    fn encoder_uses_the_shortest_exact_signed_immediate_forms() {
        let mut assembler = X86_64Assembler::new();
        assert!(assembler.add_immediate(X86_64Register::R13, 127));
        assert!(assembler.subtract_immediate(X86_64Register::R14, -129));
        assert!(assembler.xor_immediate(X86_64Register::R15, -1));
        assert!(assembler.and_immediate(X86_64Register::R12, 127));
        assert!(assembler.compare_immediate(X86_64Register::R11, -129));
        assert!(assembler.multiply_immediate(X86_64Register::R13, X86_64Register::R11, 3,));
        assert!(assembler.multiply_immediate(X86_64Register::R14, X86_64Register::R13, -129,));
        assert_eq!(
            &*assembler.finish(),
            &[
                0x49, 0x83, 0xc5, 0x7f, // ADD R13, 127 (imm8)
                0x49, 0x81, 0xee, 0x7f, 0xff, 0xff, 0xff, // SUB R14, -129 (imm32)
                0x49, 0x83, 0xf7, 0xff, // XOR R15, -1 (imm8)
                0x49, 0x83, 0xe4, 0x7f, // AND R12, 127 (imm8)
                0x49, 0x81, 0xfb, 0x7f, 0xff, 0xff, 0xff, // CMP R11, -129 (imm32)
                0x4d, 0x6b, 0xeb, 0x03, // IMUL R13, R11, 3 (imm8)
                0x4d, 0x69, 0xf5, 0x7f, 0xff, 0xff, 0xff, // IMUL R14, R13, -129
            ]
        );

        let mut too_wide = X86_64Assembler::new();
        assert!(!too_wide.add_immediate(X86_64Register::RAX, 1_i64 << 40));
        assert!(too_wide.finish().is_empty());
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
    fn range_proven_polling_keeps_three_recurrences_resident_and_publishes_them() {
        let mut operations = [NativeStraightLongOperation::Unused;
            super::super::NATIVE_STRAIGHT_LONG_MAX_OPERATIONS];
        operations[0] = NativeStraightLongOperation::BinaryAssign {
            kind: ScalarLongOpKind::Add,
            lhs: QuickLongOperand::Slot(1),
            rhs: QuickLongOperand::Slot(0),
            result: 4,
            destination: 1,
        };
        operations[1] = NativeStraightLongOperation::BinaryAssign {
            kind: ScalarLongOpKind::Add,
            lhs: QuickLongOperand::Slot(2),
            rhs: QuickLongOperand::Const(2),
            result: 5,
            destination: 2,
        };
        operations[2] = NativeStraightLongOperation::BinaryAssign {
            kind: ScalarLongOpKind::BitwiseXor,
            lhs: QuickLongOperand::Slot(3),
            rhs: QuickLongOperand::Slot(0),
            result: 6,
            destination: 3,
        };
        let publication_mask = (1u64 << 1) | (1u64 << 2) | (1u64 << 3);
        let program =
            CompiledX86StraightLongLoop::compile_range_proven_polling_with_publication_and_carried(
                NativeStraightLongLoopConfig {
                    induction_slot: 0,
                    bound: QuickLongOperand::Const(5_000),
                    operations,
                    operation_count: 3,
                    post_result: None,
                },
                publication_mask,
                publication_mask,
            )
            .unwrap();
        let mut slots = [0_i64; 64];
        slots[1] = 5;
        slots[2] = 7;
        slots[3] = 9;

        let interrupted = program.call_proven_polling(&mut slots, &true).unwrap();
        assert_eq!(
            interrupted.outcome,
            NativeStraightLongLoopOutcome::ChunkExhausted
        );
        let mut expected_xor = 9;
        for value in 0..1_024 {
            expected_xor ^= value;
        }
        assert_eq!(&slots[..4], &[1_024, 523_781, 2_055, expected_xor]);
        assert_eq!(&slots[4..7], &[0, 0, 0]);

        let completed = program.call_proven_polling(&mut slots, &false).unwrap();
        assert_eq!(completed.outcome, NativeStraightLongLoopOutcome::Completed);
        for value in 1_024..5_000 {
            expected_xor ^= value;
        }
        assert_eq!(slots[0], 5_000);
        assert_eq!(slots[1], 12_497_505);
        assert_eq!(slots[2], 10_007);
        assert_eq!(slots[3], expected_xor);
        assert_eq!(&slots[4..7], &[0, 0, 0]);
    }

    #[test]
    fn constant_bound_frees_rcx_for_a_fourth_resident_value() {
        let mut operations = [NativeStraightLongOperation::Unused;
            super::super::NATIVE_STRAIGHT_LONG_MAX_OPERATIONS];
        for (index, slot) in [1u16, 2, 3].into_iter().enumerate() {
            operations[index] = NativeStraightLongOperation::BinaryAssign {
                kind: ScalarLongOpKind::Add,
                lhs: QuickLongOperand::Slot(slot),
                rhs: QuickLongOperand::Slot(7),
                result: slot + 3,
                destination: slot,
            };
        }
        let publication_mask = (1u64 << 1) | (1u64 << 2) | (1u64 << 3);
        let program =
            CompiledX86StraightLongLoop::compile_range_proven_polling_with_publication_and_carried(
                NativeStraightLongLoopConfig {
                    induction_slot: 0,
                    bound: QuickLongOperand::Const(4),
                    operations,
                    operation_count: 3,
                    post_result: None,
                },
                publication_mask,
                publication_mask,
            )
            .unwrap();
        let mut slots = [0_i64; 64];
        slots[1..=3].copy_from_slice(&[1, 2, 3]);
        slots[7] = 5;

        let completed = program.call_proven_polling(&mut slots, &false).unwrap();
        assert_eq!(completed.outcome, NativeStraightLongLoopOutcome::Completed);
        assert_eq!(&slots[..4], &[4, 21, 22, 23]);

        let polling_code = &program.code()[program.polling_entry_offset..];
        let slot_7_rcx_load = [0x48, 0x8b, 0x8f, 0x38, 0x00, 0x00, 0x00];
        assert_eq!(
            polling_code
                .windows(slot_7_rcx_load.len())
                .filter(|window| *window == slot_7_rcx_load)
                .count(),
            1,
            "the freed bound register should cache invariant slot 7 once"
        );
        assert_eq!(
            polling_code
                .windows(4)
                .filter(|window| *window == [0x49, 0x83, 0xfb, 0x04])
                .count(),
            2,
            "entry and backedge should compare induction against the embedded bound"
        );
        for direct_add in [[0x4c, 0x03, 0xe9], [0x4c, 0x03, 0xf1], [0x4c, 0x03, 0xf9]] {
            assert!(
                polling_code
                    .windows(direct_add.len())
                    .any(|window| window == direct_add),
                "each carried recurrence should consume invariant RCX directly"
            );
        }
    }

    #[test]
    fn wide_constant_bound_keeps_the_dedicated_bound_register() {
        assert_eq!(
            x86_embedded_loop_bound(QuickLongOperand::Const(i64::from(i32::MAX) + 1)),
            None
        );
        assert_eq!(
            x86_embedded_loop_bound(QuickLongOperand::Const(i64::from(i32::MIN) - 1)),
            None
        );
        assert_eq!(
            x86_embedded_loop_bound(QuickLongOperand::Const(i64::from(i32::MAX))),
            Some(i64::from(i32::MAX))
        );
        assert_eq!(x86_embedded_loop_bound(QuickLongOperand::Slot(1)), None);
    }

    #[test]
    fn range_proven_structured_polling_merges_carried_register_values() {
        let mut operations = [NativeStraightLongOperation::Unused;
            super::super::NATIVE_STRAIGHT_LONG_MAX_OPERATIONS];
        operations[0] = NativeStraightLongOperation::BranchUnless {
            kind: ScalarLongConditionKind::LessThan,
            lhs: NativeStraightLongConditionOperand::Source(QuickLongOperand::Slot(3)),
            rhs: NativeStraightLongConditionOperand::Source(QuickLongOperand::Const(2)),
            false_target: 2,
        };
        operations[1] = NativeStraightLongOperation::BinaryAssign {
            kind: ScalarLongOpKind::Add,
            lhs: QuickLongOperand::Slot(1),
            rhs: QuickLongOperand::Slot(0),
            result: 2,
            destination: 1,
        };
        operations[2] = NativeStraightLongOperation::BinaryAssign {
            kind: ScalarLongOpKind::Add,
            lhs: QuickLongOperand::Slot(3),
            rhs: QuickLongOperand::Const(1),
            result: 4,
            destination: 3,
        };
        let publication_mask = (1u64 << 1) | (1u64 << 3);
        let program =
            CompiledX86StraightLongLoop::compile_range_proven_polling_with_publication_and_carried(
                NativeStraightLongLoopConfig {
                    induction_slot: 0,
                    bound: QuickLongOperand::Const(5_000),
                    operations,
                    operation_count: 3,
                    post_result: None,
                },
                publication_mask,
                publication_mask,
            )
            .unwrap();
        let mut slots = [0_i64; 64];
        slots[1] = 10;
        slots[3] = -5;

        let interrupted = program.call_proven_polling(&mut slots, &true).unwrap();
        assert_eq!(
            interrupted.outcome,
            NativeStraightLongLoopOutcome::ChunkExhausted
        );
        assert_eq!((slots[0], slots[1], slots[3]), (1_024, 31, 1_019));

        let completed = program.call_proven_polling(&mut slots, &false).unwrap();
        assert_eq!(completed.outcome, NativeStraightLongLoopOutcome::Completed);
        assert_eq!((slots[0], slots[1], slots[3]), (5_000, 31, 4_995));
    }

    #[test]
    fn range_proven_structured_polling_forwards_branch_local_temporaries() {
        let mut operations = [NativeStraightLongOperation::Unused;
            super::super::NATIVE_STRAIGHT_LONG_MAX_OPERATIONS];
        operations[0] = NativeStraightLongOperation::BranchUnless {
            kind: ScalarLongConditionKind::LessThan,
            lhs: NativeStraightLongConditionOperand::Source(QuickLongOperand::Slot(0)),
            rhs: NativeStraightLongConditionOperand::Source(QuickLongOperand::Const(2)),
            false_target: 4,
        };
        operations[1] = NativeStraightLongOperation::Binary {
            kind: ScalarLongOpKind::Multiply,
            lhs: QuickLongOperand::Slot(0),
            rhs: QuickLongOperand::Const(3),
            result: 5,
        };
        operations[2] = NativeStraightLongOperation::Binary {
            kind: ScalarLongOpKind::Add,
            lhs: QuickLongOperand::Slot(5),
            rhs: QuickLongOperand::Const(7),
            result: 6,
        };
        operations[3] = NativeStraightLongOperation::BinaryAssign {
            kind: ScalarLongOpKind::Add,
            lhs: QuickLongOperand::Slot(1),
            rhs: QuickLongOperand::Slot(6),
            result: 2,
            destination: 1,
        };
        operations[4] = NativeStraightLongOperation::BinaryAssign {
            kind: ScalarLongOpKind::Add,
            lhs: QuickLongOperand::Slot(3),
            rhs: QuickLongOperand::Const(1),
            result: 4,
            destination: 3,
        };
        let publication_mask = (1u64 << 1) | (1u64 << 3);
        let program =
            CompiledX86StraightLongLoop::compile_range_proven_polling_with_publication_and_carried(
                NativeStraightLongLoopConfig {
                    induction_slot: 0,
                    bound: QuickLongOperand::Const(4),
                    operations,
                    operation_count: 5,
                    post_result: None,
                },
                publication_mask,
                publication_mask,
            )
            .unwrap();
        let mut slots = [0_i64; 64];
        slots[1] = 10;
        slots[3] = -5;
        slots[5] = 77;
        slots[6] = 88;

        let completed = program.call_proven_polling(&mut slots, &false).unwrap();
        assert_eq!(completed.outcome, NativeStraightLongLoopOutcome::Completed);
        assert_eq!((slots[0], slots[1], slots[3]), (4, 27, -1));
        assert_eq!((slots[5], slots[6]), (77, 88));
    }

    #[test]
    fn range_proven_structured_polling_defers_visible_phi_publication() {
        let mut operations = [NativeStraightLongOperation::Unused;
            super::super::NATIVE_STRAIGHT_LONG_MAX_OPERATIONS];
        operations[0] = NativeStraightLongOperation::BranchUnless {
            kind: ScalarLongConditionKind::LessThan,
            lhs: NativeStraightLongConditionOperand::Source(QuickLongOperand::Slot(0)),
            rhs: NativeStraightLongConditionOperand::Source(QuickLongOperand::Const(2)),
            false_target: 4,
        };
        operations[1] = NativeStraightLongOperation::Binary {
            kind: ScalarLongOpKind::Multiply,
            lhs: QuickLongOperand::Slot(0),
            rhs: QuickLongOperand::Const(3),
            result: 5,
        };
        operations[2] = NativeStraightLongOperation::BinaryAssign {
            kind: ScalarLongOpKind::Add,
            lhs: QuickLongOperand::Slot(5),
            rhs: QuickLongOperand::Const(1),
            result: 2,
            destination: 1,
        };
        operations[3] = NativeStraightLongOperation::Jump { target: 6 };
        operations[4] = NativeStraightLongOperation::Binary {
            kind: ScalarLongOpKind::Multiply,
            lhs: QuickLongOperand::Slot(0),
            rhs: QuickLongOperand::Const(5),
            result: 6,
        };
        operations[5] = NativeStraightLongOperation::BinaryAssign {
            kind: ScalarLongOpKind::Subtract,
            lhs: QuickLongOperand::Slot(6),
            rhs: QuickLongOperand::Const(2),
            result: 2,
            destination: 1,
        };
        operations[6] = NativeStraightLongOperation::Binary {
            kind: ScalarLongOpKind::Multiply,
            lhs: QuickLongOperand::Slot(1),
            rhs: QuickLongOperand::Const(3),
            result: 7,
        };
        operations[7] = NativeStraightLongOperation::BinaryAssign {
            kind: ScalarLongOpKind::Add,
            lhs: QuickLongOperand::Slot(7),
            rhs: QuickLongOperand::Const(11),
            result: 4,
            destination: 3,
        };
        // Result/destination aliases are defined by the same operation and
        // therefore share one fixed publication register per pair.
        let publication_mask = (1u64 << 1) | (1u64 << 2) | (1u64 << 3) | (1u64 << 4);
        let program =
            CompiledX86StraightLongLoop::compile_range_proven_polling_with_publication_and_carried(
                NativeStraightLongLoopConfig {
                    induction_slot: 0,
                    bound: QuickLongOperand::Const(4),
                    operations,
                    operation_count: 8,
                    post_result: None,
                },
                publication_mask,
                0,
            )
            .unwrap();
        let mut slots = [0_i64; 64];
        let completed = program.call_proven_polling(&mut slots, &false).unwrap();
        assert_eq!(completed.outcome, NativeStraightLongLoopOutcome::Completed);
        assert_eq!(
            (slots[0], slots[1], slots[2], slots[3], slots[4]),
            (4, 13, 13, 50, 50)
        );

        let polling_code = &program.code()[program.polling_entry_offset..];
        for eliminated_copy in [[0x4c, 0x8b, 0xe8], [0x4c, 0x8b, 0xf0]] {
            assert!(
                !polling_code
                    .windows(eliminated_copy.len())
                    .any(|window| window == eliminated_copy),
                "structured result should be generated directly in its fixed register"
            );
        }
        for eliminated_forward in [[0x49, 0x8b, 0xd5], [0x49, 0x8b, 0xd6]] {
            assert!(
                !polling_code
                    .windows(eliminated_forward.len())
                    .any(|window| window == eliminated_forward),
                "fully represented fixed result should not be copied to RDX"
            );
        }
        for direct_arithmetic in [
            [0x49, 0x83, 0xc5, 0x01],
            [0x49, 0x83, 0xed, 0x02],
            [0x49, 0x83, 0xc6, 0x0b],
        ] {
            assert!(
                polling_code
                    .windows(direct_arithmetic.len())
                    .any(|window| window == direct_arithmetic),
                "expected immediate arithmetic with a fixed publication destination"
            );
        }
        for slot in [1_i32, 2_i32, 3_i32, 4_i32] {
            let mut rax_store = vec![0x48, 0x89, 0x87];
            rax_store.extend_from_slice(&(slot * 8).to_le_bytes());
            assert!(
                !polling_code
                    .windows(rax_store.len())
                    .any(|window| window == rax_store),
                "visible phi slot {slot} should publish from its fixed register"
            );
        }

        slots[1..=4].copy_from_slice(&[101, 102, 103, 104]);
        let empty = program.call_proven_polling(&mut slots, &false).unwrap();
        assert_eq!(empty.outcome, NativeStraightLongLoopOutcome::Completed);
        assert_eq!(&slots[1..=4], &[101, 102, 103, 104]);
    }

    #[test]
    fn range_proven_direct_result_preserves_old_right_resident() {
        let mut operations = [NativeStraightLongOperation::Unused;
            super::super::NATIVE_STRAIGHT_LONG_MAX_OPERATIONS];
        operations[0] = NativeStraightLongOperation::BinaryAssign {
            kind: ScalarLongOpKind::Subtract,
            lhs: QuickLongOperand::Slot(0),
            rhs: QuickLongOperand::Slot(1),
            result: 2,
            destination: 1,
        };
        let program =
            CompiledX86StraightLongLoop::compile_range_proven_polling_with_publication_and_carried(
                NativeStraightLongLoopConfig {
                    induction_slot: 0,
                    bound: QuickLongOperand::Const(4),
                    operations,
                    operation_count: 1,
                    post_result: None,
                },
                (1u64 << 1) | (1u64 << 2),
                1u64 << 1,
            )
            .unwrap();
        let mut slots = [0_i64; 64];
        slots[1] = 1;
        let completed = program.call_proven_polling(&mut slots, &false).unwrap();
        assert_eq!(completed.outcome, NativeStraightLongLoopOutcome::Completed);
        assert_eq!((slots[0], slots[1], slots[2]), (4, 3, 3));

        let polling_code = &program.code()[program.polling_entry_offset..];
        assert!(
            polling_code
                .windows(3)
                .any(|window| window == [0x4d, 0x2b, 0xe8]),
            "subtract should write directly to R13"
        );
        assert!(
            !polling_code
                .windows(3)
                .any(|window| window == [0x4c, 0x8b, 0xe8]),
            "direct subtract should not copy RAX into R13"
        );
        assert!(
            !polling_code
                .windows(3)
                .any(|window| window == [0x49, 0x8b, 0xd5]),
            "dead local result should not be forwarded from R13 to RDX"
        );
    }

    #[test]
    fn range_proven_direct_result_forwards_untracked_immediate_alias() {
        let mut operations = [NativeStraightLongOperation::Unused;
            super::super::NATIVE_STRAIGHT_LONG_MAX_OPERATIONS];
        operations[0] = NativeStraightLongOperation::BinaryAssign {
            kind: ScalarLongOpKind::Add,
            lhs: QuickLongOperand::Slot(1),
            rhs: QuickLongOperand::Slot(0),
            result: 2,
            destination: 1,
        };
        operations[1] = NativeStraightLongOperation::Move {
            source: QuickLongOperand::Slot(2),
            result: 3,
        };
        let program =
            CompiledX86StraightLongLoop::compile_range_proven_polling_with_publication_and_carried(
                NativeStraightLongLoopConfig {
                    induction_slot: 0,
                    bound: QuickLongOperand::Const(4),
                    operations,
                    operation_count: 2,
                    post_result: None,
                },
                (1u64 << 1) | (1u64 << 2) | (1u64 << 3),
                1u64 << 1,
            )
            .unwrap();
        let mut slots = [0_i64; 64];
        slots[1] = 1;
        slots[2] = 99;
        let completed = program.call_proven_polling(&mut slots, &false).unwrap();
        assert_eq!(completed.outcome, NativeStraightLongLoopOutcome::Completed);
        assert_eq!((slots[0], slots[1], slots[2], slots[3]), (4, 7, 7, 7));

        let polling_code = &program.code()[program.polling_entry_offset..];
        assert!(
            polling_code
                .windows(3)
                .any(|window| window == [0x49, 0x8b, 0xd5]),
            "untracked result alias should be forwarded from R13 to RDX"
        );
    }

    #[test]
    fn range_proven_resident_operands_feed_branch_and_rebank_fixed_arithmetic() {
        let mut operations = [NativeStraightLongOperation::Unused;
            super::super::NATIVE_STRAIGHT_LONG_MAX_OPERATIONS];
        operations[0] = NativeStraightLongOperation::BranchUnless {
            kind: ScalarLongConditionKind::LessThan,
            lhs: NativeStraightLongConditionOperand::Source(QuickLongOperand::Slot(1)),
            rhs: NativeStraightLongConditionOperand::Source(QuickLongOperand::Slot(2)),
            false_target: 2,
        };
        operations[1] = NativeStraightLongOperation::BinaryAssign {
            kind: ScalarLongOpKind::Add,
            lhs: QuickLongOperand::Slot(1),
            rhs: QuickLongOperand::Slot(2),
            result: 3,
            destination: 1,
        };
        operations[2] = NativeStraightLongOperation::BinaryAssign {
            kind: ScalarLongOpKind::Add,
            lhs: QuickLongOperand::Slot(2),
            rhs: QuickLongOperand::Const(1),
            result: 4,
            destination: 2,
        };
        let publication_mask = (1u64 << 1) | (1u64 << 2);
        let program =
            CompiledX86StraightLongLoop::compile_range_proven_polling_with_publication_and_carried(
                NativeStraightLongLoopConfig {
                    induction_slot: 0,
                    bound: QuickLongOperand::Const(4),
                    operations,
                    operation_count: 3,
                    post_result: None,
                },
                publication_mask,
                publication_mask,
            )
            .unwrap();
        let mut slots = [0_i64; 64];
        slots[1] = 1;
        slots[2] = 2;
        let completed = program.call_proven_polling(&mut slots, &false).unwrap();
        assert_eq!(completed.outcome, NativeStraightLongLoopOutcome::Completed);
        assert_eq!((slots[0], slots[1], slots[2]), (4, 7, 6));

        let polling_code = &program.code()[program.polling_entry_offset..];
        let initial_jge = polling_code
            .windows(2)
            .position(|window| window == [0x0f, 0x8d])
            .expect("polling entry should reject an empty range");
        let mut loop_offset = initial_jge + 6;
        while polling_code.get(loop_offset) == Some(&0x90) {
            loop_offset += 1;
        }
        assert_eq!(
            (program.polling_entry_offset + loop_offset) % X86_STRUCTURED_LOOP_ALIGNMENT,
            0,
            "structured polling loop should start on its cache-line boundary"
        );
        assert!(
            polling_code
                .windows(3)
                .any(|window| window == [0x4d, 0x3b, 0xee]),
            "branch should compare R13 and R14 directly"
        );
        assert!(
            polling_code
                .windows(3)
                .any(|window| window == [0x4d, 0x8b, 0xc6]),
            "fixed-to-fixed arithmetic should re-bank R14 through R8"
        );
        assert!(
            polling_code
                .windows(3)
                .any(|window| window == [0x4d, 0x03, 0xe8]),
            "re-banked add should write R13 from R8"
        );
    }

    #[test]
    fn range_proven_resident_rhs_feeds_scratch_result_directly() {
        let mut operations = [NativeStraightLongOperation::Unused;
            super::super::NATIVE_STRAIGHT_LONG_MAX_OPERATIONS];
        operations[0] = NativeStraightLongOperation::Binary {
            kind: ScalarLongOpKind::Add,
            lhs: QuickLongOperand::Slot(0),
            rhs: QuickLongOperand::Slot(1),
            result: 2,
        };
        let program =
            CompiledX86StraightLongLoop::compile_range_proven_polling_with_publication_and_carried(
                NativeStraightLongLoopConfig {
                    induction_slot: 0,
                    bound: QuickLongOperand::Const(4),
                    operations,
                    operation_count: 1,
                    post_result: None,
                },
                (1u64 << 1) | (1u64 << 2),
                1u64 << 1,
            )
            .unwrap();
        let mut slots = [0_i64; 64];
        slots[1] = 5;
        let completed = program.call_proven_polling(&mut slots, &false).unwrap();
        assert_eq!(completed.outcome, NativeStraightLongLoopOutcome::Completed);
        assert_eq!((slots[0], slots[1], slots[2]), (4, 5, 8));

        let polling_code = &program.code()[program.polling_entry_offset..];
        assert!(
            polling_code
                .windows(3)
                .any(|window| window == [0x49, 0x03, 0xc5]),
            "scratch result should consume resident R13 directly"
        );
        assert!(
            !polling_code
                .windows(3)
                .any(|window| window == [0x4d, 0x8b, 0xc5]),
            "resident R13 should not be copied into R8 for an RAX result"
        );
    }

    #[test]
    fn range_proven_division_moves_latest_rdx_divisor_before_cqo() {
        let mut operations = [NativeStraightLongOperation::Unused;
            super::super::NATIVE_STRAIGHT_LONG_MAX_OPERATIONS];
        operations[0] = NativeStraightLongOperation::Binary {
            kind: ScalarLongOpKind::Add,
            lhs: QuickLongOperand::Slot(0),
            rhs: QuickLongOperand::Const(1),
            result: 2,
        };
        operations[1] = NativeStraightLongOperation::Binary {
            kind: ScalarLongOpKind::IntDivide,
            lhs: QuickLongOperand::Const(100),
            rhs: QuickLongOperand::Slot(2),
            result: 3,
        };
        let program =
            CompiledX86StraightLongLoop::compile_range_proven_polling_with_publication_and_carried(
                NativeStraightLongLoopConfig {
                    induction_slot: 0,
                    bound: QuickLongOperand::Const(4),
                    operations,
                    operation_count: 2,
                    post_result: None,
                },
                1u64 << 3,
                0,
            )
            .unwrap();
        let mut slots = [0_i64; 64];
        let completed = program.call_proven_polling(&mut slots, &false).unwrap();
        assert_eq!(completed.outcome, NativeStraightLongLoopOutcome::Completed);
        assert_eq!((slots[0], slots[3]), (4, 25));

        let polling_code = &program.code()[program.polling_entry_offset..];
        assert!(
            polling_code
                .windows(5)
                .any(|window| window == [0x4c, 0x8b, 0xc2, 0x48, 0x99]),
            "RDX divisor must move to R8 immediately before CQO"
        );
    }

    #[test]
    fn structured_phi_rejects_nonlocal_read_before_merge() {
        let mut operations = [NativeStraightLongOperation::Unused;
            super::super::NATIVE_STRAIGHT_LONG_MAX_OPERATIONS];
        operations[0] = NativeStraightLongOperation::BranchUnless {
            kind: ScalarLongConditionKind::LessThan,
            lhs: NativeStraightLongConditionOperand::Source(QuickLongOperand::Slot(0)),
            rhs: NativeStraightLongConditionOperand::Source(QuickLongOperand::Const(2)),
            false_target: 3,
        };
        operations[1] = NativeStraightLongOperation::Move {
            source: QuickLongOperand::Const(1),
            result: 1,
        };
        operations[2] = NativeStraightLongOperation::Jump { target: 4 };
        operations[3] = NativeStraightLongOperation::Move {
            source: QuickLongOperand::Const(9),
            result: 5,
        };
        operations[4] = NativeStraightLongOperation::Move {
            source: QuickLongOperand::Slot(1),
            result: 2,
        };
        operations[5] = NativeStraightLongOperation::Move {
            source: QuickLongOperand::Const(2),
            result: 1,
        };
        let config = NativeStraightLongLoopConfig {
            induction_slot: 0,
            bound: QuickLongOperand::Const(4),
            operations,
            operation_count: 6,
            post_result: None,
        };
        let block_starts = straight_long_structured_block_starts(&config);
        let (definitely_written_before, definitely_written_exit) =
            straight_long_structured_definitely_written(&config);
        assert_ne!(definitely_written_exit & (1u64 << 1), 0);
        assert!(!structured_phi_candidate_is_safe(
            &config,
            1u64 << 1,
            &block_starts,
            &definitely_written_before,
        ));
    }

    #[test]
    fn structured_lowering_executes_both_forward_control_flow_edges() {
        let program = CompiledX86StraightLongLoop::compile(structured_recurrence(4)).unwrap();
        let mut slots = [0_i64; 64];
        let result = program.call(&mut slots).unwrap();
        assert_eq!(result.outcome, NativeStraightLongLoopOutcome::Completed);
        assert_eq!(&slots[..3], &[4, 224, 224]);

        assert_eq!(
            program
                .code()
                .windows(5)
                .filter(|window| *window == [0x49, 0x83, 0xfb, 0x02, 0x7d])
                .count(),
            5,
            "each ABI entry should use short JGE for the structured false edge"
        );
        assert_eq!(
            program.code().iter().filter(|byte| **byte == 0xeb).count(),
            5,
            "each ABI entry should use a short unconditional join jump"
        );
    }

    #[test]
    fn structured_lowering_elides_control_flow_to_the_immediate_successor() {
        let mut config = composed_add_recurrence(4);
        config.operations[0] = NativeStraightLongOperation::BranchUnless {
            kind: ScalarLongConditionKind::Equal,
            lhs: NativeStraightLongConditionOperand::Source(QuickLongOperand::Slot(0)),
            rhs: NativeStraightLongConditionOperand::Source(QuickLongOperand::Const(-1)),
            false_target: 1,
        };
        config.operations[1] = NativeStraightLongOperation::Jump { target: 2 };
        config.operations[2] = NativeStraightLongOperation::BinaryAssign {
            kind: ScalarLongOpKind::Add,
            lhs: QuickLongOperand::Slot(1),
            rhs: QuickLongOperand::Slot(0),
            result: 2,
            destination: 1,
        };
        config.operation_count = 3;
        let program = CompiledX86StraightLongLoop::compile(config).unwrap();
        let mut slots = [0_i64; 64];
        let result = program.call(&mut slots).unwrap();
        assert_eq!(result.outcome, NativeStraightLongLoopOutcome::Completed);
        assert_eq!(&slots[..3], &[4, 6, 6]);

        let fast_code = &program.code()[..program.checked_entry_offset];
        assert!(
            !fast_code.windows(2).any(|window| window == [0x0f, 0x85]),
            "a predicate whose false edge is fallthrough should not be emitted"
        );
        assert!(
            !fast_code.contains(&0xe9),
            "an unconditional jump to fallthrough should not be emitted"
        );
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
    fn checked_operations_share_cold_side_exit_publication() {
        let mut config = composed_add_recurrence(1);
        config.operations[0] = NativeStraightLongOperation::Binary {
            kind: ScalarLongOpKind::IntDivide,
            lhs: QuickLongOperand::Slot(0),
            rhs: QuickLongOperand::Slot(7),
            result: 4,
        };
        config.operations[1] = NativeStraightLongOperation::BinaryAssign {
            kind: ScalarLongOpKind::Add,
            lhs: QuickLongOperand::Slot(1),
            rhs: QuickLongOperand::Const(1),
            result: 2,
            destination: 1,
        };
        config.operation_count = 2;
        config.post_result = None;
        let program = CompiledX86StraightLongLoop::compile(config).unwrap();

        let mut divide_by_zero = [0_i64; 64];
        divide_by_zero[7] = 0;
        assert_eq!(
            program.call(&mut divide_by_zero).unwrap(),
            NativeStraightLongLoopResult {
                outcome: NativeStraightLongLoopOutcome::OperationSideExit,
                failed_operation: Some(0),
            }
        );
        let mut sum_overflow = [0_i64; 64];
        sum_overflow[1] = i64::MAX;
        sum_overflow[7] = 1;
        assert_eq!(
            program.call(&mut sum_overflow).unwrap(),
            NativeStraightLongLoopResult {
                outcome: NativeStraightLongLoopOutcome::OperationSideExit,
                failed_operation: Some(1),
            }
        );

        let checked_code =
            &program.code()[program.checked_entry_offset..program.chunk_entry_offset];
        for selector in [
            [0xb8, 0x06, 0x00, 0x00, 0x00, 0xe9],
            [0xb8, 0x06, 0x01, 0x00, 0x00, 0xe9],
        ] {
            assert_eq!(
                checked_code
                    .windows(selector.len())
                    .filter(|window| *window == selector)
                    .count(),
                1,
                "each failed operation should select one shared cold epilogue"
            );
        }
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
        assert!(
            program
                .code()
                .windows(4)
                .any(|window| window == [0x48, 0x83, 0xe0, 0x01]),
            "small remainder mask should encode directly in AND"
        );
        let mut divisor_load = vec![0x49, 0xb8];
        divisor_load.extend_from_slice(&2_i64.to_le_bytes());
        assert!(
            !program
                .code()
                .windows(divisor_load.len())
                .any(|window| window == divisor_load),
            "power-of-two divisor should not be materialized before mask lowering"
        );
    }

    #[test]
    fn wide_power_of_two_remainder_materializes_only_the_exact_mask() {
        let divisor = 1_i64 << 40;
        let mask = divisor - 1;
        let mut config = composed_add_recurrence(1);
        config.operations[0] = NativeStraightLongOperation::Modulo {
            value: QuickLongOperand::Slot(6),
            divisor,
            result: 4,
        };
        config.operation_count = 1;
        config.post_result = None;
        let program = CompiledX86StraightLongLoop::compile(config).unwrap();
        let mut slots = [0_i64; 64];
        slots[6] = -(divisor + 5);
        let result = program.call(&mut slots).unwrap();
        assert_eq!(result.outcome, NativeStraightLongLoopOutcome::Completed);
        assert_eq!(slots[4], -5);

        let mut mask_load = vec![0x49, 0xb8];
        mask_load.extend_from_slice(&mask.to_le_bytes());
        assert!(
            program
                .code()
                .windows(mask_load.len())
                .any(|window| window == mask_load),
            "mask outside sign-extended imm32 must retain MOVABS fallback"
        );
        let mut divisor_load = vec![0x49, 0xb8];
        divisor_load.extend_from_slice(&divisor.to_le_bytes());
        assert!(
            !program
                .code()
                .windows(divisor_load.len())
                .any(|window| window == divisor_load),
            "recognized divisor itself is dead even when the mask needs MOVABS"
        );
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
        assert!(
            program.code().windows(4).any(|window| {
                matches!(window[0], 0x48 | 0x49)
                    && window[1] == 0x83
                    && window[2] & 0xf8 == 0xf8
                    && window[3] == 0
            }),
            "comparison against zero should use CMP r64, imm8"
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
