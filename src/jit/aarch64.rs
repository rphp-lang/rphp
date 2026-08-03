use crate::vm::function::{ScalarLongFunctionPlan, ScalarLongOpKind, ScalarLongSource};
use crate::vm::quick::{QuickLongAccumulateLoop, QuickLongOperand, QuickLongTerm};
use std::cell::{Cell, OnceCell};
use std::ffi::{c_int, c_void};
use std::fmt;
use std::io;
use std::mem::MaybeUninit;
use std::ptr::NonNull;

const PROT_READ: c_int = 0x01;
const PROT_WRITE: c_int = 0x02;
const PROT_EXEC: c_int = 0x04;
const MAP_PRIVATE: c_int = 0x0002;
const MAP_ANONYMOUS: c_int = 0x1000;

unsafe extern "C" {
    fn mmap(
        address: *mut c_void,
        length: usize,
        protection: c_int,
        flags: c_int,
        file_descriptor: c_int,
        offset: i64,
    ) -> *mut c_void;
    fn mprotect(address: *mut c_void, length: usize, protection: c_int) -> c_int;
    fn munmap(address: *mut c_void, length: usize) -> c_int;
    fn getpagesize() -> c_int;
    fn sys_icache_invalidate(start: *mut c_void, length: usize);
}

/// General-purpose ARM64 register accepted by the prototype encoder.
///
/// Register 31 is deliberately excluded because its meaning depends on the
/// instruction (`SP` or the zero register). Encodings that need `XZR` insert it
/// explicitly instead of making that ambiguity part of the public API.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Arm64Register(u8);

impl Arm64Register {
    pub const X0: Self = Self(0);
    pub const X1: Self = Self(1);
    pub const X2: Self = Self(2);

    #[inline]
    const fn bits(self) -> u32 {
        self.0 as u32
    }

    #[inline]
    const fn from_code(code: u8) -> Self {
        debug_assert!(code < 31);
        Self(code)
    }
}

#[derive(Debug, Clone, Copy)]
#[repr(u8)]
enum Arm64Condition {
    Equal = 0,
    NotEqual = 1,
    Overflow = 6,
    GreaterOrEqual = 10,
}

/// Small binary ARM64 encoder. It emits instruction words directly and never
/// invokes a textual assembler, linker, LLVM, Cranelift, or DynASM.
#[derive(Debug, Default)]
pub struct Arm64Assembler {
    words: Vec<u32>,
}

impl Arm64Assembler {
    pub fn new() -> Self {
        Self::default()
    }

    /// Encode `ADD Xd, Xn, Xm` (64-bit, unshifted register form).
    pub fn add_register(
        &mut self,
        destination: Arm64Register,
        lhs: Arm64Register,
        rhs: Arm64Register,
    ) {
        let instruction = 0x8b00_0000 | (rhs.bits() << 16) | (lhs.bits() << 5) | destination.bits();
        self.words.push(instruction);
    }

    /// Encode the `MUL Xd, Xn, Xm` alias of `MADD Xd, Xn, Xm, XZR`.
    pub fn multiply_register(
        &mut self,
        destination: Arm64Register,
        lhs: Arm64Register,
        rhs: Arm64Register,
    ) {
        const XZR: u32 = 31;
        let instruction =
            0x9b00_0000 | (rhs.bits() << 16) | (XZR << 10) | (lhs.bits() << 5) | destination.bits();
        self.words.push(instruction);
    }

    /// Encode `ADDS Xd, Xn, Xm`, retaining signed-overflow information in PSTATE.V.
    fn add_register_checked(
        &mut self,
        destination: Arm64Register,
        lhs: Arm64Register,
        rhs: Arm64Register,
    ) {
        let instruction = 0xab00_0000 | (rhs.bits() << 16) | (lhs.bits() << 5) | destination.bits();
        self.words.push(instruction);
    }

    /// Encode `SUBS Xd, Xn, Xm`, retaining signed-overflow information in PSTATE.V.
    fn subtract_register_checked(
        &mut self,
        destination: Arm64Register,
        lhs: Arm64Register,
        rhs: Arm64Register,
    ) {
        let instruction = 0xeb00_0000 | (rhs.bits() << 16) | (lhs.bits() << 5) | destination.bits();
        self.words.push(instruction);
    }

    /// Encode `EOR Xd, Xn, Xm`.
    fn exclusive_or_register(
        &mut self,
        destination: Arm64Register,
        lhs: Arm64Register,
        rhs: Arm64Register,
    ) {
        let instruction = 0xca00_0000 | (rhs.bits() << 16) | (lhs.bits() << 5) | destination.bits();
        self.words.push(instruction);
    }

    /// Encode `SMULH Xd, Xn, Xm`, the signed high half of a 128-bit product.
    fn signed_multiply_high(
        &mut self,
        destination: Arm64Register,
        lhs: Arm64Register,
        rhs: Arm64Register,
    ) {
        let instruction = 0x9b40_7c00 | (rhs.bits() << 16) | (lhs.bits() << 5) | destination.bits();
        self.words.push(instruction);
    }

    /// Encode `SDIV Xd, Xn, Xm`.
    fn signed_divide(
        &mut self,
        destination: Arm64Register,
        lhs: Arm64Register,
        rhs: Arm64Register,
    ) {
        let instruction = 0x9ac0_0c00 | (rhs.bits() << 16) | (lhs.bits() << 5) | destination.bits();
        self.words.push(instruction);
    }

    /// Encode `MSUB Xd, Xn, Xm, Xa`: `Xa - (Xn * Xm)`.
    fn multiply_subtract(
        &mut self,
        destination: Arm64Register,
        multiplicand: Arm64Register,
        multiplier: Arm64Register,
        minuend: Arm64Register,
    ) {
        let instruction = 0x9b00_8000
            | (multiplier.bits() << 16)
            | (minuend.bits() << 10)
            | (multiplicand.bits() << 5)
            | destination.bits();
        self.words.push(instruction);
    }

    /// Encode the `ASR Xd, Xn, #shift` alias of `SBFM`.
    fn arithmetic_shift_right(
        &mut self,
        destination: Arm64Register,
        source: Arm64Register,
        shift: u8,
    ) {
        debug_assert!(shift < 64);
        let instruction = 0x9340_0000
            | ((shift as u32) << 16)
            | (63 << 10)
            | (source.bits() << 5)
            | destination.bits();
        self.words.push(instruction);
    }

    /// Encode the `CMP Xn, Xm` alias of `SUBS XZR, Xn, Xm`.
    fn compare_registers(&mut self, lhs: Arm64Register, rhs: Arm64Register) {
        const XZR: u32 = 31;
        let instruction = 0xeb00_0000 | (rhs.bits() << 16) | (lhs.bits() << 5) | XZR;
        self.words.push(instruction);
    }

    /// Encode `CMP Xn, XZR` without exposing register 31 through the API.
    fn compare_with_zero(&mut self, value: Arm64Register) {
        const XZR: u32 = 31;
        let instruction = 0xeb00_0000 | (XZR << 16) | (value.bits() << 5) | XZR;
        self.words.push(instruction);
    }

    /// Encode the `MOV Xd, Xm` alias of `ORR Xd, XZR, Xm`.
    fn move_register(&mut self, destination: Arm64Register, source: Arm64Register) {
        let instruction = 0xaa00_03e0 | (source.bits() << 16) | destination.bits();
        self.words.push(instruction);
    }

    /// Encode `LDR Xt, [Xn, #offset]` using the scaled unsigned-immediate form.
    fn load_u64(&mut self, destination: Arm64Register, base: Arm64Register, offset: u16) {
        debug_assert_eq!(offset % 8, 0);
        let scaled_offset = u32::from(offset / 8);
        debug_assert!(scaled_offset < 4096);
        let instruction =
            0xf940_0000 | (scaled_offset << 10) | (base.bits() << 5) | destination.bits();
        self.words.push(instruction);
    }

    /// Encode `STR Xt, [Xn, #offset]` using the scaled unsigned-immediate form.
    fn store_u64(&mut self, source: Arm64Register, base: Arm64Register, offset: u16) {
        debug_assert_eq!(offset % 8, 0);
        let scaled_offset = u32::from(offset / 8);
        debug_assert!(scaled_offset < 4096);
        let instruction = 0xf900_0000 | (scaled_offset << 10) | (base.bits() << 5) | source.bits();
        self.words.push(instruction);
    }

    /// Materialize an arbitrary 64-bit immediate with one `MOVZ` and the
    /// necessary `MOVK` instructions.
    fn move_immediate(&mut self, destination: Arm64Register, value: i64) {
        let value = value as u64;
        let low = value as u16;
        self.words
            .push(0xd280_0000 | (u32::from(low) << 5) | destination.bits());
        for halfword in 1..4 {
            let part = ((value >> (halfword * 16)) & 0xffff) as u16;
            if part == 0 {
                continue;
            }
            self.words.push(
                0xf280_0000
                    | ((halfword as u32) << 21)
                    | (u32::from(part) << 5)
                    | destination.bits(),
            );
        }
    }

    /// Emit a `B.cond` whose displacement will be patched once its target is
    /// known. Both forward exits and backward loop edges use this relocation.
    fn conditional_branch_placeholder(&mut self, condition: Arm64Condition) -> usize {
        let word = self.words.len();
        self.words.push(0x5400_0000 | condition as u32);
        word
    }

    fn patch_conditional_branch(&mut self, word: usize, target: usize) -> bool {
        let displacement = target as isize - word as isize;
        if !(-262_144..=262_143).contains(&displacement) {
            return false;
        }
        let immediate = (displacement as i32 as u32) & 0x7ffff;
        let condition = self.words[word] & 0xf;
        self.words[word] = 0x5400_0000 | (immediate << 5) | condition;
        true
    }

    #[inline]
    fn word_count(&self) -> usize {
        self.words.len()
    }

    /// Encode `RET X30`, the standard return from a leaf function.
    pub fn ret(&mut self) {
        self.words.push(0xd65f_03c0);
    }

    /// Return the exact little-endian instruction bytes that will be executed.
    pub fn finish(self) -> Vec<u8> {
        let mut code = Vec::with_capacity(self.words.len() * size_of::<u32>());
        for word in self.words {
            code.extend_from_slice(&word.to_le_bytes());
        }
        code
    }
}

struct ExecutableMemory {
    address: NonNull<u8>,
    mapped_length: usize,
}

impl ExecutableMemory {
    fn from_code(code: &[u8]) -> io::Result<Self> {
        if code.is_empty() {
            return Err(io::Error::new(
                io::ErrorKind::InvalidInput,
                "cannot execute an empty code buffer",
            ));
        }

        let page_size = unsafe { getpagesize() };
        if page_size <= 0 {
            return Err(io::Error::last_os_error());
        }
        let page_size = page_size as usize;
        let mapped_length = code
            .len()
            .checked_next_multiple_of(page_size)
            .ok_or_else(|| {
                io::Error::new(io::ErrorKind::InvalidInput, "code buffer is too large")
            })?;

        // Maintain W^X: the mapping is writable while populated and executable
        // only after all bytes have been copied into it.
        let raw = unsafe {
            mmap(
                std::ptr::null_mut(),
                mapped_length,
                PROT_READ | PROT_WRITE,
                MAP_PRIVATE | MAP_ANONYMOUS,
                -1,
                0,
            )
        };
        if raw == (-1_isize) as *mut c_void {
            return Err(io::Error::last_os_error());
        }
        let Some(address) = NonNull::new(raw.cast::<u8>()) else {
            unsafe {
                munmap(raw, mapped_length);
            }
            return Err(io::Error::other("mmap returned a null address"));
        };

        unsafe {
            std::ptr::copy_nonoverlapping(code.as_ptr(), address.as_ptr(), code.len());
            sys_icache_invalidate(address.as_ptr().cast::<c_void>(), code.len());
        }

        if unsafe { mprotect(raw, mapped_length, PROT_READ | PROT_EXEC) } != 0 {
            let error = io::Error::last_os_error();
            unsafe {
                munmap(raw, mapped_length);
            }
            return Err(error);
        }

        Ok(Self {
            address,
            mapped_length,
        })
    }

    #[inline]
    fn entry(&self) -> *const u8 {
        self.address.as_ptr()
    }
}

impl Drop for ExecutableMemory {
    fn drop(&mut self) {
        unsafe {
            munmap(self.address.as_ptr().cast::<c_void>(), self.mapped_length);
        }
    }
}

/// Native prototype for `(first + second) * multiplier`.
///
/// This object owns the executable mapping, so the entry point cannot outlive
/// its code. Overflow behavior is intentionally not PHP-compatible yet; the
/// prototype is isolated from the VM and only proves encoding, memory, and ABI.
pub struct CompiledAddMultiply {
    memory: ExecutableMemory,
    code: Box<[u8]>,
}

impl CompiledAddMultiply {
    pub fn compile() -> io::Result<Self> {
        let mut assembler = Arm64Assembler::new();
        assembler.add_register(Arm64Register::X0, Arm64Register::X0, Arm64Register::X1);
        assembler.multiply_register(Arm64Register::X0, Arm64Register::X0, Arm64Register::X2);
        assembler.ret();
        let code = assembler.finish().into_boxed_slice();
        let memory = ExecutableMemory::from_code(&code)?;
        Ok(Self { memory, code })
    }

    /// Execute the generated leaf function through the platform C ABI.
    pub fn call(&self, first: i64, second: i64, multiplier: i64) -> i64 {
        type NativeFunction = unsafe extern "C" fn(i64, i64, i64) -> i64;
        let function: NativeFunction = unsafe { std::mem::transmute(self.memory.entry()) };
        unsafe { function(first, second, multiplier) }
    }

    pub fn code(&self) -> &[u8] {
        &self.code
    }
}

impl fmt::Debug for CompiledAddMultiply {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter
            .debug_struct("CompiledAddMultiply")
            .field("code", &self.code)
            .finish_non_exhaustive()
    }
}

const NATIVE_LONG_ACCUMULATE_COMPLETED: u32 = 0;
const NATIVE_LONG_ACCUMULATE_CHUNK_EXHAUSTED: u32 = 1;
const NATIVE_LONG_ACCUMULATE_SUM_OVERFLOW: u32 = 2;
const NATIVE_LONG_ACCUMULATE_INCREMENT_OVERFLOW: u32 = 3;
const NATIVE_LONG_ACCUMULATE_TERM_OVERFLOW: u32 = 4;
const NATIVE_LONG_ACCUMULATE_CONDITION_SIDE_EXIT: u32 = 5;

/// Mutable state shared with the native accumulate-loop ABI.
///
/// The generated function owns no PHP values and cannot observe the VM. It
/// advances this scalar snapshot for at most the supplied iteration budget;
/// the VM publishes it only at an interrupt boundary, completion, or precise
/// side exit.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[repr(C)]
pub struct NativeLongAccumulateState {
    pub induction: i64,
    pub bound: i64,
    pub accumulator: i64,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum QuickLongAccumulateJitOutcome {
    Completed,
    ChunkExhausted,
    ConditionSideExit,
    TermOverflow,
    SumOverflow,
    IncrementOverflow,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct NativeConditionalLongLoopResult {
    pub outcome: QuickLongAccumulateJitOutcome,
    pub addition_executed: bool,
}

#[derive(Debug)]
pub enum QuickLongAccumulateJitError {
    InvalidProgram(&'static str),
    ZeroIterationBudget,
    BranchOutOfRange,
    Memory(io::Error),
    InvalidNativeStatus(u32),
}

impl fmt::Display for QuickLongAccumulateJitError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::InvalidProgram(reason) => {
                write!(formatter, "invalid native Long loop: {reason}")
            }
            Self::ZeroIterationBudget => {
                formatter.write_str("native loop iteration budget must be non-zero")
            }
            Self::BranchOutOfRange => {
                formatter.write_str("ARM64 loop branch is out of range")
            }
            Self::Memory(error) => write!(formatter, "cannot create executable memory: {error}"),
            Self::InvalidNativeStatus(status) => {
                write!(formatter, "native loop returned an unknown status {status}")
            }
        }
    }
}

impl std::error::Error for QuickLongAccumulateJitError {
    fn source(&self) -> Option<&(dyn std::error::Error + 'static)> {
        match self {
            Self::Memory(error) => Some(error),
            _ => None,
        }
    }
}

impl From<io::Error> for QuickLongAccumulateJitError {
    fn from(error: io::Error) -> Self {
        Self::Memory(error)
    }
}

/// Native lowering of the guarded region:
///
/// ```text
/// while induction < bound {
///     accumulator = checked_add(accumulator, induction)
///     induction = checked_add(induction, 1)
/// }
/// ```
///
/// ABI: `x0` points to `NativeLongAccumulateState`, `x1` is a non-zero
/// iteration budget, and `w0` returns a `QuickLongAccumulateJitOutcome`
/// discriminator. Checked operations publish neither their wrapped result nor
/// an ambiguous resume position.
pub struct CompiledQuickLongAccumulateLoop {
    memory: ExecutableMemory,
    code: Box<[u8]>,
}

impl CompiledQuickLongAccumulateLoop {
    pub fn compile() -> Result<Self, QuickLongAccumulateJitError> {
        Self::compile_term(None)
    }

    pub fn compile_with_addend(addend: i64) -> Result<Self, QuickLongAccumulateJitError> {
        Self::compile_term((addend != 0).then_some(addend))
    }

    fn compile_term(addend: Option<i64>) -> Result<Self, QuickLongAccumulateJitError> {
        let mut assembler = Arm64Assembler::new();
        let induction = Arm64Register::from_code(2);
        let bound = Arm64Register::from_code(3);
        let accumulator = Arm64Register::from_code(4);
        let one = Arm64Register::from_code(5);
        let addend_register = Arm64Register::from_code(6);
        let computed_term = Arm64Register::from_code(7);
        let checked_result = Arm64Register::from_code(8);

        assembler.load_u64(induction, Arm64Register::X0, 0);
        assembler.load_u64(bound, Arm64Register::X0, 8);
        assembler.load_u64(accumulator, Arm64Register::X0, 16);
        assembler.move_immediate(one, 1);
        if let Some(addend) = addend {
            assembler.move_immediate(addend_register, addend);
        }

        let loop_word = assembler.word_count();
        assembler.compare_registers(induction, bound);
        let completed_branch = assembler
            .conditional_branch_placeholder(Arm64Condition::GreaterOrEqual);

        let (term, term_overflow_branch) = if addend.is_some() {
            assembler.add_register_checked(computed_term, induction, addend_register);
            let overflow =
                assembler.conditional_branch_placeholder(Arm64Condition::Overflow);
            (computed_term, Some(overflow))
        } else {
            (induction, None)
        };

        // Keep the old accumulator live until the overflow branch has passed.
        assembler.add_register_checked(checked_result, accumulator, term);
        let sum_overflow_branch =
            assembler.conditional_branch_placeholder(Arm64Condition::Overflow);
        assembler.move_register(accumulator, checked_result);

        // The same transactional rule preserves the old induction value for
        // the baseline increment instruction on overflow.
        assembler.add_register_checked(checked_result, induction, one);
        let increment_overflow_branch =
            assembler.conditional_branch_placeholder(Arm64Condition::Overflow);
        assembler.move_register(induction, checked_result);

        assembler.subtract_register_checked(
            Arm64Register::X1,
            Arm64Register::X1,
            one,
        );
        let loop_branch =
            assembler.conditional_branch_placeholder(Arm64Condition::NotEqual);

        let chunk_exhausted_word = assembler.word_count();
        emit_long_accumulate_state(&mut assembler, induction, accumulator);
        assembler.move_immediate(
            Arm64Register::X0,
            i64::from(NATIVE_LONG_ACCUMULATE_CHUNK_EXHAUSTED),
        );
        assembler.ret();

        let completed_word = assembler.word_count();
        emit_long_accumulate_state(&mut assembler, induction, accumulator);
        assembler.move_immediate(
            Arm64Register::X0,
            i64::from(NATIVE_LONG_ACCUMULATE_COMPLETED),
        );
        assembler.ret();

        let sum_overflow_word = assembler.word_count();
        emit_long_accumulate_state(&mut assembler, induction, accumulator);
        assembler.move_immediate(
            Arm64Register::X0,
            i64::from(NATIVE_LONG_ACCUMULATE_SUM_OVERFLOW),
        );
        assembler.ret();

        let increment_overflow_word = assembler.word_count();
        emit_long_accumulate_state(&mut assembler, induction, accumulator);
        assembler.move_immediate(
            Arm64Register::X0,
            i64::from(NATIVE_LONG_ACCUMULATE_INCREMENT_OVERFLOW),
        );
        assembler.ret();

        let term_overflow_word = term_overflow_branch.map(|_| {
            let word = assembler.word_count();
            emit_long_accumulate_state(&mut assembler, induction, accumulator);
            assembler.move_immediate(
                Arm64Register::X0,
                i64::from(NATIVE_LONG_ACCUMULATE_TERM_OVERFLOW),
            );
            assembler.ret();
            word
        });

        for (branch, target) in [
            (completed_branch, completed_word),
            (sum_overflow_branch, sum_overflow_word),
            (increment_overflow_branch, increment_overflow_word),
            (loop_branch, loop_word),
        ] {
            if !assembler.patch_conditional_branch(branch, target) {
                return Err(QuickLongAccumulateJitError::BranchOutOfRange);
            }
        }
        if let Some((branch, target)) = term_overflow_branch.zip(term_overflow_word)
            && !assembler.patch_conditional_branch(branch, target)
        {
            return Err(QuickLongAccumulateJitError::BranchOutOfRange);
        }
        debug_assert!(chunk_exhausted_word < completed_word);

        let code = assembler.finish().into_boxed_slice();
        let memory = ExecutableMemory::from_code(&code)?;
        Ok(Self { memory, code })
    }

    pub fn call(
        &self,
        state: &mut NativeLongAccumulateState,
        iteration_budget: u64,
    ) -> Result<QuickLongAccumulateJitOutcome, QuickLongAccumulateJitError> {
        if iteration_budget == 0 {
            return Err(QuickLongAccumulateJitError::ZeroIterationBudget);
        }
        type NativeFunction =
            unsafe extern "C" fn(*mut NativeLongAccumulateState, u64) -> u32;
        let function: NativeFunction = unsafe { std::mem::transmute(self.memory.entry()) };
        let status = unsafe { function(state, iteration_budget) };
        match status {
            NATIVE_LONG_ACCUMULATE_COMPLETED => {
                Ok(QuickLongAccumulateJitOutcome::Completed)
            }
            NATIVE_LONG_ACCUMULATE_CHUNK_EXHAUSTED => {
                Ok(QuickLongAccumulateJitOutcome::ChunkExhausted)
            }
            NATIVE_LONG_ACCUMULATE_TERM_OVERFLOW => {
                Ok(QuickLongAccumulateJitOutcome::TermOverflow)
            }
            NATIVE_LONG_ACCUMULATE_SUM_OVERFLOW => {
                Ok(QuickLongAccumulateJitOutcome::SumOverflow)
            }
            NATIVE_LONG_ACCUMULATE_INCREMENT_OVERFLOW => {
                Ok(QuickLongAccumulateJitOutcome::IncrementOverflow)
            }
            status => Err(QuickLongAccumulateJitError::InvalidNativeStatus(status)),
        }
    }

    pub fn code(&self) -> &[u8] {
        &self.code
    }
}

fn emit_long_accumulate_state(
    assembler: &mut Arm64Assembler,
    induction: Arm64Register,
    accumulator: Arm64Register,
) {
    assembler.store_u64(induction, Arm64Register::X0, 0);
    assembler.store_u64(accumulator, Arm64Register::X0, 16);
}

/// Lazy native cache attached to one already-hot quick-loop plan. Cloning a
/// compiler plan intentionally starts with an empty cache; executable mappings
/// and profile counters are runtime state rather than compiler metadata.
pub struct QuickLongAccumulateJitCache {
    compiled: OnceCell<Option<CompiledQuickLongAccumulateLoop>>,
    native_entries: Cell<u64>,
    native_chunks: Cell<u64>,
    side_exits: Cell<u64>,
}

impl QuickLongAccumulateJitCache {
    pub const fn new() -> Self {
        Self {
            compiled: OnceCell::new(),
            native_entries: Cell::new(0),
            native_chunks: Cell::new(0),
            side_exits: Cell::new(0),
        }
    }

    pub fn dispatch_chunk(
        &self,
        plan: &QuickLongAccumulateLoop,
        state: &mut NativeLongAccumulateState,
        iteration_budget: u64,
    ) -> Option<Result<QuickLongAccumulateJitOutcome, QuickLongAccumulateJitError>> {
        let program = self
            .compiled
            .get_or_init(|| match plan.term {
                QuickLongTerm::Induction => CompiledQuickLongAccumulateLoop::compile().ok(),
                QuickLongTerm::InductionPlusConst { addend, .. } => {
                    CompiledQuickLongAccumulateLoop::compile_with_addend(addend).ok()
                }
                _ => None,
            })
            .as_ref()?;
        self.native_chunks
            .set(self.native_chunks.get().saturating_add(1));
        let outcome = program.call(state, iteration_budget);
        if matches!(
            outcome,
            Ok(QuickLongAccumulateJitOutcome::TermOverflow)
                | Ok(QuickLongAccumulateJitOutcome::SumOverflow)
                | Ok(QuickLongAccumulateJitOutcome::IncrementOverflow)
                | Err(_)
        ) {
            self.side_exits.set(self.side_exits.get().saturating_add(1));
        }
        Some(outcome)
    }

    pub fn record_region_entry(&self) {
        self.native_entries
            .set(self.native_entries.get().saturating_add(1));
    }

    pub fn is_compiled(&self) -> bool {
        matches!(self.compiled.get(), Some(Some(_)))
    }

    pub fn native_entries(&self) -> u64 {
        self.native_entries.get()
    }

    pub fn native_chunks(&self) -> u64 {
        self.native_chunks.get()
    }

    pub fn side_exits(&self) -> u64 {
        self.side_exits.get()
    }
}

impl Default for QuickLongAccumulateJitCache {
    fn default() -> Self {
        Self::new()
    }
}

impl Clone for QuickLongAccumulateJitCache {
    fn clone(&self) -> Self {
        Self::new()
    }
}

impl fmt::Debug for QuickLongAccumulateJitCache {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter
            .debug_struct("QuickLongAccumulateJitCache")
            .field("compiled", &self.is_compiled())
            .field("native_entries", &self.native_entries())
            .field("native_chunks", &self.native_chunks())
            .field("side_exits", &self.side_exits())
            .finish()
    }
}

/// Compile-time description of the first general `QuickLongOpsLoop` subset.
/// Slot operands are loop-invariant; only the induction and accumulator slots
/// are mutated by native code.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum NativeConditionalLongLoopCondition {
    LessThan {
        rhs: QuickLongOperand,
    },
    ModuloEqual {
        divisor: i64,
        rhs: QuickLongOperand,
    },
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct NativeConditionalLongLoopConfig {
    pub induction_slot: u16,
    pub bound: QuickLongOperand,
    pub condition: NativeConditionalLongLoopCondition,
    pub accumulator_slot: u16,
}

#[derive(Debug, Default)]
#[repr(C)]
struct NativeConditionalLongLoopControl {
    addition_executed: u64,
}

/// Whole-region ARM64 lowering for a conditional accumulator expressed by the
/// general typed loop IR rather than `QuickLongAccumulateLoop`:
///
/// ```text
/// while induction < bound {
///     if condition(induction) {
///         accumulator = checked_add(accumulator, induction)
///     }
///     induction = checked_add(induction, 1)
/// }
/// ```
///
/// `condition` is either `induction < invariant` or
/// `(induction % constant) == invariant`. The latter guards the two values for
/// which AArch64 `SDIV` does not have PHP's observable behaviour and leaves
/// through a precise condition side exit.
pub struct CompiledQuickLongConditionalAccumulateLoop {
    memory: ExecutableMemory,
    code: Box<[u8]>,
    config: NativeConditionalLongLoopConfig,
}

impl CompiledQuickLongConditionalAccumulateLoop {
    pub fn compile(
        config: NativeConditionalLongLoopConfig,
    ) -> Result<Self, QuickLongAccumulateJitError> {
        validate_conditional_long_loop_config(config)?;

        let mut assembler = Arm64Assembler::new();
        let induction = Arm64Register::from_code(2);
        let bound = Arm64Register::from_code(3);
        let condition_operand = Arm64Register::from_code(4);
        let accumulator = Arm64Register::from_code(5);
        let one = Arm64Register::from_code(6);
        let checked_result = Arm64Register::from_code(7);
        let condition_rhs = Arm64Register::from_code(8);
        let quotient = Arm64Register::from_code(9);
        let remainder = Arm64Register::from_code(10);
        let addition_executed = Arm64Register::from_code(11);
        let control = Arm64Register::from_code(12);
        let guard = Arm64Register::from_code(13);

        // x2 carries the control pointer at the ABI boundary, but becomes the
        // induction register inside the leaf function.
        assembler.move_register(control, Arm64Register::X2);
        assembler.load_u64(
            induction,
            Arm64Register::X0,
            long_slot_offset(config.induction_slot),
        );
        emit_native_long_operand(&mut assembler, config.bound, bound);
        match config.condition {
            NativeConditionalLongLoopCondition::LessThan { rhs } => {
                emit_native_long_operand(&mut assembler, rhs, condition_operand);
            }
            NativeConditionalLongLoopCondition::ModuloEqual { divisor, rhs } => {
                assembler.move_immediate(condition_operand, divisor);
                emit_native_long_operand(&mut assembler, rhs, condition_rhs);
            }
        }
        assembler.load_u64(
            accumulator,
            Arm64Register::X0,
            long_slot_offset(config.accumulator_slot),
        );
        assembler.move_immediate(one, 1);
        assembler.move_immediate(addition_executed, 0);

        let loop_word = assembler.word_count();
        assembler.compare_registers(induction, bound);
        let completed_branch = assembler
            .conditional_branch_placeholder(Arm64Condition::GreaterOrEqual);

        let mut condition_side_exit_branches = Vec::new();
        let skip_add_branch = match config.condition {
            NativeConditionalLongLoopCondition::LessThan { .. } => {
                assembler.compare_registers(induction, condition_operand);
                Some(assembler.conditional_branch_placeholder(Arm64Condition::GreaterOrEqual))
            }
            NativeConditionalLongLoopCondition::ModuloEqual { divisor: 0, .. } => {
                // The header must win for an empty loop. Once the body is
                // reached, replay the canonical Mod instruction so PHP emits
                // its normal division-by-zero error.
                assembler.compare_registers(induction, induction);
                condition_side_exit_branches
                    .push(assembler.conditional_branch_placeholder(Arm64Condition::Equal));
                None
            }
            NativeConditionalLongLoopCondition::ModuloEqual { divisor, .. } => {
                if divisor == -1 {
                    assembler.move_immediate(guard, i64::MIN);
                    assembler.compare_registers(induction, guard);
                    condition_side_exit_branches
                        .push(assembler.conditional_branch_placeholder(Arm64Condition::Equal));
                }
                assembler.signed_divide(quotient, induction, condition_operand);
                assembler.multiply_subtract(
                    remainder,
                    quotient,
                    condition_operand,
                    induction,
                );
                assembler.compare_registers(remainder, condition_rhs);
                Some(assembler.conditional_branch_placeholder(Arm64Condition::NotEqual))
            }
        };
        assembler.add_register_checked(checked_result, accumulator, induction);
        let sum_overflow_branch =
            assembler.conditional_branch_placeholder(Arm64Condition::Overflow);
        assembler.move_register(accumulator, checked_result);
        assembler.move_immediate(addition_executed, 1);

        let increment_word = assembler.word_count();
        assembler.add_register_checked(checked_result, induction, one);
        let increment_overflow_branch =
            assembler.conditional_branch_placeholder(Arm64Condition::Overflow);
        assembler.move_register(induction, checked_result);
        assembler.subtract_register_checked(
            Arm64Register::X1,
            Arm64Register::X1,
            one,
        );
        let loop_branch =
            assembler.conditional_branch_placeholder(Arm64Condition::NotEqual);

        emit_conditional_long_loop_state(
            &mut assembler,
            config,
            induction,
            accumulator,
            control,
            addition_executed,
        );
        assembler.move_immediate(
            Arm64Register::X0,
            i64::from(NATIVE_LONG_ACCUMULATE_CHUNK_EXHAUSTED),
        );
        assembler.ret();

        let completed_word = assembler.word_count();
        emit_conditional_long_loop_state(
            &mut assembler,
            config,
            induction,
            accumulator,
            control,
            addition_executed,
        );
        assembler.move_immediate(
            Arm64Register::X0,
            i64::from(NATIVE_LONG_ACCUMULATE_COMPLETED),
        );
        assembler.ret();

        let sum_overflow_word = assembler.word_count();
        emit_conditional_long_loop_state(
            &mut assembler,
            config,
            induction,
            accumulator,
            control,
            addition_executed,
        );
        assembler.move_immediate(
            Arm64Register::X0,
            i64::from(NATIVE_LONG_ACCUMULATE_SUM_OVERFLOW),
        );
        assembler.ret();

        let increment_overflow_word = assembler.word_count();
        emit_conditional_long_loop_state(
            &mut assembler,
            config,
            induction,
            accumulator,
            control,
            addition_executed,
        );
        assembler.move_immediate(
            Arm64Register::X0,
            i64::from(NATIVE_LONG_ACCUMULATE_INCREMENT_OVERFLOW),
        );
        assembler.ret();

        let condition_side_exit_word = if condition_side_exit_branches.is_empty() {
            None
        } else {
            let word = assembler.word_count();
            emit_conditional_long_loop_state(
                &mut assembler,
                config,
                induction,
                accumulator,
                control,
                addition_executed,
            );
            assembler.move_immediate(
                Arm64Register::X0,
                i64::from(NATIVE_LONG_ACCUMULATE_CONDITION_SIDE_EXIT),
            );
            assembler.ret();
            Some(word)
        };

        for (branch, target) in [
            (completed_branch, completed_word),
            (sum_overflow_branch, sum_overflow_word),
            (increment_overflow_branch, increment_overflow_word),
            (loop_branch, loop_word),
        ] {
            if !assembler.patch_conditional_branch(branch, target) {
                return Err(QuickLongAccumulateJitError::BranchOutOfRange);
            }
        }
        if let Some(branch) = skip_add_branch
            && !assembler.patch_conditional_branch(branch, increment_word)
        {
            return Err(QuickLongAccumulateJitError::BranchOutOfRange);
        }
        if let Some(target) = condition_side_exit_word {
            for branch in condition_side_exit_branches {
                if !assembler.patch_conditional_branch(branch, target) {
                    return Err(QuickLongAccumulateJitError::BranchOutOfRange);
                }
            }
        }

        let code = assembler.finish().into_boxed_slice();
        let memory = ExecutableMemory::from_code(&code)?;
        Ok(Self {
            memory,
            code,
            config,
        })
    }

    pub fn call(
        &self,
        slots: &mut [i64; 64],
        iteration_budget: u64,
    ) -> Result<NativeConditionalLongLoopResult, QuickLongAccumulateJitError> {
        if iteration_budget == 0 {
            return Err(QuickLongAccumulateJitError::ZeroIterationBudget);
        }
        type NativeFunction = unsafe extern "C" fn(
            *mut i64,
            u64,
            *mut NativeConditionalLongLoopControl,
        ) -> u32;
        let function: NativeFunction = unsafe { std::mem::transmute(self.memory.entry()) };
        let mut control = NativeConditionalLongLoopControl::default();
        let status = unsafe { function(slots.as_mut_ptr(), iteration_budget, &mut control) };
        let outcome = match status {
            NATIVE_LONG_ACCUMULATE_COMPLETED => QuickLongAccumulateJitOutcome::Completed,
            NATIVE_LONG_ACCUMULATE_CHUNK_EXHAUSTED => {
                QuickLongAccumulateJitOutcome::ChunkExhausted
            }
            NATIVE_LONG_ACCUMULATE_CONDITION_SIDE_EXIT => {
                QuickLongAccumulateJitOutcome::ConditionSideExit
            }
            NATIVE_LONG_ACCUMULATE_SUM_OVERFLOW => QuickLongAccumulateJitOutcome::SumOverflow,
            NATIVE_LONG_ACCUMULATE_INCREMENT_OVERFLOW => {
                QuickLongAccumulateJitOutcome::IncrementOverflow
            }
            status => return Err(QuickLongAccumulateJitError::InvalidNativeStatus(status)),
        };
        Ok(NativeConditionalLongLoopResult {
            outcome,
            addition_executed: control.addition_executed != 0,
        })
    }

    pub fn config(&self) -> NativeConditionalLongLoopConfig {
        self.config
    }

    pub fn code(&self) -> &[u8] {
        &self.code
    }
}

fn validate_conditional_long_loop_config(
    config: NativeConditionalLongLoopConfig,
) -> Result<(), QuickLongAccumulateJitError> {
    if config.induction_slot >= 64 || config.accumulator_slot >= 64 {
        return Err(QuickLongAccumulateJitError::InvalidProgram(
            "mutable slot is outside the native slot ABI",
        ));
    }
    if config.induction_slot == config.accumulator_slot {
        return Err(QuickLongAccumulateJitError::InvalidProgram(
            "induction and accumulator slots must be distinct",
        ));
    }
    let condition_rhs = match config.condition {
        NativeConditionalLongLoopCondition::LessThan { rhs }
        | NativeConditionalLongLoopCondition::ModuloEqual { rhs, .. } => rhs,
    };
    for operand in [config.bound, condition_rhs] {
        if let QuickLongOperand::Slot(slot) = operand {
            if slot >= 64 {
                return Err(QuickLongAccumulateJitError::InvalidProgram(
                    "invariant operand slot is outside the native slot ABI",
                ));
            }
            if slot == config.induction_slot || slot == config.accumulator_slot {
                return Err(QuickLongAccumulateJitError::InvalidProgram(
                    "an invariant operand aliases mutable loop state",
                ));
            }
        }
    }
    Ok(())
}

#[inline]
fn long_slot_offset(slot: u16) -> u16 {
    debug_assert!(slot < 64);
    slot * 8
}

fn emit_native_long_operand(
    assembler: &mut Arm64Assembler,
    operand: QuickLongOperand,
    destination: Arm64Register,
) {
    match operand {
        QuickLongOperand::Slot(slot) => assembler.load_u64(
            destination,
            Arm64Register::X0,
            long_slot_offset(slot),
        ),
        QuickLongOperand::Const(value) => assembler.move_immediate(destination, value),
    }
}

fn emit_conditional_long_loop_state(
    assembler: &mut Arm64Assembler,
    config: NativeConditionalLongLoopConfig,
    induction: Arm64Register,
    accumulator: Arm64Register,
    control: Arm64Register,
    addition_executed: Arm64Register,
) {
    assembler.store_u64(
        induction,
        Arm64Register::X0,
        long_slot_offset(config.induction_slot),
    );
    assembler.store_u64(
        accumulator,
        Arm64Register::X0,
        long_slot_offset(config.accumulator_slot),
    );
    assembler.store_u64(addition_executed, control, 0);
}

pub struct QuickLongOpsJitCache {
    compiled: OnceCell<Option<CompiledQuickLongConditionalAccumulateLoop>>,
    native_entries: Cell<u64>,
    native_chunks: Cell<u64>,
    side_exits: Cell<u64>,
}

impl QuickLongOpsJitCache {
    pub const fn new() -> Self {
        Self {
            compiled: OnceCell::new(),
            native_entries: Cell::new(0),
            native_chunks: Cell::new(0),
            side_exits: Cell::new(0),
        }
    }

    pub fn dispatch_chunk(
        &self,
        config: NativeConditionalLongLoopConfig,
        slots: &mut [i64; 64],
        iteration_budget: u64,
    ) -> Result<NativeConditionalLongLoopResult, QuickLongAccumulateJitError> {
        let Some(program) = self
            .compiled
            .get_or_init(|| {
                CompiledQuickLongConditionalAccumulateLoop::compile(config).ok()
            })
            .as_ref()
        else {
            return Err(QuickLongAccumulateJitError::InvalidProgram(
                "conditional loop could not be compiled",
            ));
        };
        debug_assert_eq!(program.config(), config);
        self.native_chunks
            .set(self.native_chunks.get().saturating_add(1));
        let outcome = program.call(slots, iteration_budget);
        if match &outcome {
            Ok(result) => matches!(
                result.outcome,
                QuickLongAccumulateJitOutcome::ConditionSideExit
                    | QuickLongAccumulateJitOutcome::SumOverflow
                    | QuickLongAccumulateJitOutcome::IncrementOverflow
            ),
            Err(_) => true,
        } {
            self.side_exits.set(self.side_exits.get().saturating_add(1));
        }
        outcome
    }

    pub fn record_region_entry(&self) {
        self.native_entries
            .set(self.native_entries.get().saturating_add(1));
    }

    pub fn is_compiled(&self) -> bool {
        matches!(self.compiled.get(), Some(Some(_)))
    }

    pub fn native_entries(&self) -> u64 {
        self.native_entries.get()
    }

    pub fn native_chunks(&self) -> u64 {
        self.native_chunks.get()
    }

    pub fn side_exits(&self) -> u64 {
        self.side_exits.get()
    }
}

impl Default for QuickLongOpsJitCache {
    fn default() -> Self {
        Self::new()
    }
}

impl Clone for QuickLongOpsJitCache {
    fn clone(&self) -> Self {
        Self::new()
    }
}

impl fmt::Debug for QuickLongOpsJitCache {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter
            .debug_struct("QuickLongOpsJitCache")
            .field("compiled", &self.is_compiled())
            .field("native_entries", &self.native_entries())
            .field("native_chunks", &self.native_chunks())
            .field("side_exits", &self.side_exits())
            .finish()
    }
}

const MAX_SCALAR_LONG_INPUTS: usize = 8;
const MAX_SCALAR_LONG_OPERATIONS: usize = 8;
const FIRST_TEMPORARY_REGISTER: u8 = 9;
const NATIVE_STATUS_SUCCESS: u32 = 0;
const NATIVE_STATUS_SIDE_EXIT: u32 = 1;

#[derive(Debug)]
pub enum ScalarLongJitError {
    InvalidProgram(&'static str),
    UnsupportedOperation(ScalarLongOpKind),
    BranchOutOfRange,
    Memory(io::Error),
    InputCount { expected: usize, actual: usize },
    InvalidNativeStatus(u32),
}

impl fmt::Display for ScalarLongJitError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::InvalidProgram(reason) => {
                write!(formatter, "invalid scalar Long program: {reason}")
            }
            Self::UnsupportedOperation(operation) => {
                write!(
                    formatter,
                    "unsupported scalar Long JIT operation: {operation:?}"
                )
            }
            Self::BranchOutOfRange => formatter.write_str("ARM64 side-exit branch is out of range"),
            Self::Memory(error) => write!(formatter, "cannot create executable memory: {error}"),
            Self::InputCount { expected, actual } => {
                write!(
                    formatter,
                    "JIT expected {expected} inputs but received {actual}"
                )
            }
            Self::InvalidNativeStatus(status) => {
                write!(formatter, "JIT returned an unknown status {status}")
            }
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

pub const SCALAR_LONG_JIT_HOT_THRESHOLD: u16 = 64;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ScalarLongJitDispatch {
    Interpret,
    Value(i64),
    SideExit,
}

/// Per-plan lazy native-code cache. RPHP's VM is deliberately single-threaded,
/// so `Cell` and `OnceCell` match the existing FunctionCommon hotness model and
/// add no atomic operations to the call path.
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
        arguments: &[i64; 8],
    ) -> ScalarLongJitDispatch {
        // A one-op leaf is already inlined by the no-JIT executor. Paying an
        // indirect native call for it would move in the wrong direction.
        if plan.select.is_some() || plan.program.operations.len() < 2 {
            return ScalarLongJitDispatch::Interpret;
        }

        if self.compiled.get().is_none() {
            let calls = self.calls.get().saturating_add(1);
            self.calls.set(calls);
            if calls < SCALAR_LONG_JIT_HOT_THRESHOLD {
                return ScalarLongJitDispatch::Interpret;
            }
            let compiled = CompiledScalarLongProgram::compile(plan).ok();
            let _ = self.compiled.set(compiled);
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

/// An isolated native lowering of RPHP's existing straight-line integer IR.
///
/// ABI: `x0` points to an input array, `x1` points to one output slot, and `w0`
/// returns zero on success or one when checked arithmetic requires canonical
/// PHP execution. The output is written only on success.
pub struct CompiledScalarLongProgram {
    memory: ExecutableMemory,
    code: Box<[u8]>,
    input_count: usize,
}

impl CompiledScalarLongProgram {
    pub fn compile(plan: &ScalarLongFunctionPlan) -> Result<Self, ScalarLongJitError> {
        validate_scalar_long_plan(plan)?;

        let mut assembler = Arm64Assembler::new();
        let mut side_exit_branches = Vec::new();

        for (index, operation) in plan.program.operations.iter().copied().enumerate() {
            let lhs =
                emit_scalar_source(&mut assembler, operation.lhs, Arm64Register::from_code(2));
            let rhs =
                emit_scalar_source(&mut assembler, operation.rhs, Arm64Register::from_code(3));
            let destination = scalar_temporary_register(index);

            match operation.kind {
                ScalarLongOpKind::Add => {
                    assembler.add_register_checked(destination, lhs, rhs);
                    side_exit_branches
                        .push(assembler.conditional_branch_placeholder(Arm64Condition::Overflow));
                }
                ScalarLongOpKind::Subtract => {
                    assembler.subtract_register_checked(destination, lhs, rhs);
                    side_exit_branches
                        .push(assembler.conditional_branch_placeholder(Arm64Condition::Overflow));
                }
                ScalarLongOpKind::Multiply => {
                    assembler.multiply_register(destination, lhs, rhs);
                    assembler.signed_multiply_high(Arm64Register::from_code(2), lhs, rhs);
                    assembler.arithmetic_shift_right(Arm64Register::from_code(3), destination, 63);
                    assembler.compare_registers(
                        Arm64Register::from_code(2),
                        Arm64Register::from_code(3),
                    );
                    side_exit_branches
                        .push(assembler.conditional_branch_placeholder(Arm64Condition::NotEqual));
                }
                ScalarLongOpKind::BitwiseXor => {
                    assembler.exclusive_or_register(destination, lhs, rhs);
                }
                ScalarLongOpKind::IntDivide => {
                    emit_division_guards(&mut assembler, lhs, rhs, &mut side_exit_branches)?;
                    assembler.signed_divide(destination, lhs, rhs);
                }
                ScalarLongOpKind::Modulo => {
                    emit_division_guards(&mut assembler, lhs, rhs, &mut side_exit_branches)?;
                    let quotient = Arm64Register::from_code(4);
                    assembler.signed_divide(quotient, lhs, rhs);
                    assembler.multiply_subtract(destination, quotient, rhs, lhs);
                }
            }
        }

        let output = emit_scalar_source(
            &mut assembler,
            plan.program.outputs[0],
            Arm64Register::from_code(2),
        );
        assembler.store_u64(output, Arm64Register::X1, 0);
        assembler.move_immediate(Arm64Register::X0, i64::from(NATIVE_STATUS_SUCCESS));
        assembler.ret();

        let side_exit_word = assembler.word_count();
        assembler.move_immediate(Arm64Register::X0, i64::from(NATIVE_STATUS_SIDE_EXIT));
        assembler.ret();
        for branch in side_exit_branches {
            if !assembler.patch_conditional_branch(branch, side_exit_word) {
                return Err(ScalarLongJitError::BranchOutOfRange);
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

    pub fn call(&self, inputs: &[i64]) -> Result<ScalarLongJitOutcome, ScalarLongJitError> {
        if inputs.len() != self.input_count {
            return Err(ScalarLongJitError::InputCount {
                expected: self.input_count,
                actual: inputs.len(),
            });
        }

        type NativeFunction = unsafe extern "C" fn(*const i64, *mut i64) -> u32;
        let function: NativeFunction = unsafe { std::mem::transmute(self.memory.entry()) };
        let mut output = MaybeUninit::<i64>::uninit();
        let status = unsafe { function(inputs.as_ptr(), output.as_mut_ptr()) };
        match status {
            NATIVE_STATUS_SUCCESS => {
                Ok(ScalarLongJitOutcome::Value(unsafe { output.assume_init() }))
            }
            NATIVE_STATUS_SIDE_EXIT => Ok(ScalarLongJitOutcome::SideExit),
            status => Err(ScalarLongJitError::InvalidNativeStatus(status)),
        }
    }

    pub fn code(&self) -> &[u8] {
        &self.code
    }
}

fn validate_scalar_long_plan(plan: &ScalarLongFunctionPlan) -> Result<(), ScalarLongJitError> {
    if plan.select.is_some() {
        return Err(ScalarLongJitError::InvalidProgram(
            "conditional selects are not part of the first native slice",
        ));
    }
    if plan.public_args as usize > MAX_SCALAR_LONG_INPUTS {
        return Err(ScalarLongJitError::InvalidProgram(
            "too many public inputs for the prototype ABI",
        ));
    }
    if plan.program.operations.len() > MAX_SCALAR_LONG_OPERATIONS {
        return Err(ScalarLongJitError::InvalidProgram(
            "too many operations for the prototype register allocator",
        ));
    }
    if plan.program.output_count != 1 {
        return Err(ScalarLongJitError::InvalidProgram(
            "the scalar leaf must expose exactly one output",
        ));
    }

    for (index, operation) in plan.program.operations.iter().enumerate() {
        match operation.kind {
            ScalarLongOpKind::Add
            | ScalarLongOpKind::Subtract
            | ScalarLongOpKind::Multiply
            | ScalarLongOpKind::BitwiseXor
            | ScalarLongOpKind::IntDivide
            | ScalarLongOpKind::Modulo => {}
        }
        validate_scalar_source(operation.lhs, index, plan.public_args)?;
        validate_scalar_source(operation.rhs, index, plan.public_args)?;
    }
    validate_scalar_source(
        plan.program.outputs[0],
        plan.program.operations.len(),
        plan.public_args,
    )
}

fn emit_division_guards(
    assembler: &mut Arm64Assembler,
    lhs: Arm64Register,
    rhs: Arm64Register,
    side_exit_branches: &mut Vec<usize>,
) -> Result<(), ScalarLongJitError> {
    // AArch64 SDIV deliberately returns zero for a zero divisor and wraps
    // MIN / -1. RPHP's typed executor uses checked_div/checked_rem, so both
    // cases must leave native code and resume canonical PHP execution.
    assembler.compare_with_zero(rhs);
    side_exit_branches.push(assembler.conditional_branch_placeholder(Arm64Condition::Equal));

    let guard_constant = Arm64Register::from_code(4);
    assembler.move_immediate(guard_constant, -1);
    assembler.compare_registers(rhs, guard_constant);
    let not_minus_one = assembler.conditional_branch_placeholder(Arm64Condition::NotEqual);

    assembler.move_immediate(guard_constant, i64::MIN);
    assembler.compare_registers(lhs, guard_constant);
    side_exit_branches.push(assembler.conditional_branch_placeholder(Arm64Condition::Equal));

    let safe_division = assembler.word_count();
    if !assembler.patch_conditional_branch(not_minus_one, safe_division) {
        return Err(ScalarLongJitError::BranchOutOfRange);
    }
    Ok(())
}

fn validate_scalar_source(
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

fn emit_scalar_source(
    assembler: &mut Arm64Assembler,
    source: ScalarLongSource,
    scratch: Arm64Register,
) -> Arm64Register {
    match source {
        ScalarLongSource::Input(index) => {
            assembler.load_u64(scratch, Arm64Register::X0, index * 8);
            scratch
        }
        ScalarLongSource::Constant(value) => {
            assembler.move_immediate(scratch, value);
            scratch
        }
        ScalarLongSource::Temporary(index) => scalar_temporary_register(index as usize),
    }
}

#[inline]
fn scalar_temporary_register(index: usize) -> Arm64Register {
    debug_assert!(index < MAX_SCALAR_LONG_OPERATIONS);
    Arm64Register::from_code(FIRST_TEMPORARY_REGISTER + index as u8)
}
