use crate::vm::function::{ScalarLongFunctionPlan, ScalarLongOpKind, ScalarLongSource};
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
    NotEqual = 1,
    Overflow = 6,
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

    /// Emit a forward `B.cond` whose displacement will be patched once the
    /// shared side-exit label is known.
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
                ScalarLongOpKind::IntDivide | ScalarLongOpKind::Modulo => {
                    return Err(ScalarLongJitError::UnsupportedOperation(operation.kind));
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
            | ScalarLongOpKind::BitwiseXor => {}
            ScalarLongOpKind::IntDivide | ScalarLongOpKind::Modulo => {
                return Err(ScalarLongJitError::UnsupportedOperation(operation.kind));
            }
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
