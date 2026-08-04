//! Minimal x86-64 SysV backend slice.
//!
//! Like the ARM64 backend, this encoder writes machine instructions directly;
//! it does not invoke an assembler, linker or external code-generation crate.

use super::memory::ExecutableMemory;
use std::io;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct X86_64Register(u8);

impl X86_64Register {
    pub const RAX: Self = Self(0);
    pub const RDX: Self = Self(2);
    pub const RSI: Self = Self(6);
    pub const RDI: Self = Self(7);

    #[cfg(test)]
    const R8: Self = Self(8);
    #[cfg(test)]
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
}
