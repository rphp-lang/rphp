use super::opcode::OpCode;
use super::function::FunctionCommon;

/// Operand type — where to find the operand
#[repr(u8)]
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum OpType {
    Unused = 0,
    Const = 1,   // literal from OpArray.literals
    Tmp = 2,     // temporary variable
    Var = 3,     // VAR (refcounted temporary)
    Cv = 4,      // compiled variable ($a, $b, ...)
}

/// Single VM instruction — compact, 16 bytes.
///
/// Operand indices are u16 (max 65535 CVs/TMPs/literals/instructions per function).
/// Inline cache lives in OpArray's side table, indexed by instruction position.
/// 16 bytes = 4 instructions per 64-byte cache line (was 20B = 3.2/line).
#[repr(C)]
#[derive(Debug, Clone, Copy)]
pub struct Instruction {
    pub opcode: OpCode,
    pub op1_type: OpType,
    pub op2_type: OpType,
    pub result_type: OpType,
    pub op1: u16,
    pub op2: u16,
    pub result: u16,
    // 2 bytes padding to align extended_value to 4
    pub _pad: u16,
    /// Extended value (opcode-specific extra data, u32 for ForeachNext key encoding etc.)
    pub extended_value: u32,
}

impl Instruction {
    pub fn new(opcode: OpCode) -> Self {
        Self {
            opcode,
            op1_type: OpType::Unused,
            op2_type: OpType::Unused,
            result_type: OpType::Unused,
            op1: 0,
            op2: 0,
            result: 0,
            _pad: 0,
            extended_value: 0,
        }
    }
}

/// Monomorphic inline cache entry — one per instruction slot in OpArray.
/// Only InitFcall/InitMethodCall/InitStaticCall use their cache entry;
/// other instructions' entries stay zeroed (no overhead — just unused memory).
///
/// Mutated via raw pointer writes in the single-threaded VM dispatch loop.
pub struct InlineCache {
    /// Resolved function pointer (null = not cached).
    pub func: *const FunctionCommon,
    /// class_id that `func` was resolved for (methods only; 0 = function call).
    pub class_id: u32,
    /// Property access cache flags (used by FetchObjR/AssignObjProp):
    /// low 2 bits: read-safe/write-safe flags
    /// high 30 bits: declared property slot
    ///
    /// Packing keeps InlineCache at 16 bytes.
    prop_info: u32,
}

const _: [(); 16] = [(); std::mem::size_of::<InlineCache>()];

// SAFETY: InlineCache is only written from the single VM execution thread.
unsafe impl Send for InlineCache {}
unsafe impl Sync for InlineCache {}

impl InlineCache {
    const PROP_FLAG_MASK: u32 = 0b11;

    pub fn empty() -> Self {
        Self {
            func: std::ptr::null(),
            class_id: 0,
            prop_info: 0,
        }
    }

    #[inline(always)]
    pub fn property_flags(&self) -> u32 {
        self.prop_info & Self::PROP_FLAG_MASK
    }

    #[inline(always)]
    pub fn property_slot(&self) -> usize {
        (self.prop_info >> 2) as usize
    }

    #[inline]
    pub fn set_property(&mut self, class_id: u32, slot: usize, flags: u32) {
        debug_assert!(flags <= Self::PROP_FLAG_MASK);
        debug_assert!(slot <= (u32::MAX >> 2) as usize);
        self.class_id = class_id;
        self.prop_info = ((slot as u32) << 2) | flags;
    }
}
