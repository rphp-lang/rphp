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

/// Single VM instruction — compact, no cache fields.
///
/// Inline cache lives in OpArray's side table (`cache: Vec<InlineCache>`),
/// indexed by instruction position. This keeps Instruction small (~20 bytes)
/// so more opcodes fit per cache line.
#[repr(C)]
#[derive(Debug, Clone, Copy)]
pub struct Instruction {
    pub opcode: OpCode,
    pub op1_type: OpType,
    pub op2_type: OpType,
    pub result_type: OpType,
    pub op1: u32,
    pub op2: u32,
    pub result: u32,
    /// Extended value (opcode-specific extra data)
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
}

// SAFETY: InlineCache is only written from the single VM execution thread.
unsafe impl Send for InlineCache {}
unsafe impl Sync for InlineCache {}

impl InlineCache {
    pub fn empty() -> Self {
        Self {
            func: std::ptr::null(),
            class_id: 0,
        }
    }
}
