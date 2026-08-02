use super::opcode::OpCode;
use super::function::FunctionCommon;

/// InitFcall/InitMethodCall flag: every source argument is positional and at
/// least one needs opcodes between Init and its Send. A proven scalar callee may
/// therefore defer its full frame while preserving source evaluation order.
pub const CALL_FLAG_DEFERRED_SCALAR_CANDIDATE: u16 = 1;

/// InitArray flag: at least one compile-time literal string key guarantees
/// general hash storage rather than packed integer storage.
pub const ARRAY_INIT_HASH_HINT: u16 = 1;

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
/// InitFcall/InitMethodCall/InitStaticCall use their cache entry for normal
/// dispatch. DoFcall can use its otherwise-idle entry for a dynamic string
/// callback resolved by an internal callback helper.
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
    const METHOD_FUSION_ELIGIBLE: u32 = 1;
    const METHOD_LONG_PROPERTY_PLAN: u32 = 2;
    const METHOD_PROPERTY_GETTER_PLAN: u32 = 4;
    const CALLBACK_CACHE_DISABLED: *const FunctionCommon = 1usize as *const FunctionCommon;

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

    /// Last validated ordered-entry position for a `FetchDimR` string key.
    /// The array validates both the position and current key before use, so no
    /// array identity or mutation version is required for correctness.
    #[inline(always)]
    pub fn string_array_position(&self) -> Option<usize> {
        (self.prop_info != 0).then_some((self.prop_info - 1) as usize)
    }

    #[inline]
    pub fn set_string_array_position(&mut self, position: usize) {
        let encoded = u32::try_from(position)
            .ok()
            .and_then(|position| position.checked_add(1))
            .unwrap_or(0);
        self.func = std::ptr::null();
        self.class_id = 0;
        self.prop_info = encoded;
    }

    /// Cache constructor resolution for a `NewObj` site. A null function with
    /// a non-zero class ID is a valid negative cache entry for classes without
    /// `__construct`.
    #[inline]
    pub fn set_constructor(&mut self, func: *const FunctionCommon, class_id: u32) {
        debug_assert!(class_id != 0);
        self.func = func;
        self.class_id = class_id;
        self.prop_info = 0;
    }

    /// Cache a monomorphic method resolution and whether its already-proven
    /// FastScalar body is short enough to benefit from InitMethodCall fusion.
    #[inline]
    pub fn set_method(
        &mut self,
        func: *const FunctionCommon,
        class_id: u32,
        fusion_eligible: bool,
        long_property_plan: bool,
        property_getter_plan: bool,
    ) {
        self.func = func;
        self.class_id = class_id;
        self.prop_info = (if fusion_eligible { Self::METHOD_FUSION_ELIGIBLE } else { 0 })
            | (if long_property_plan { Self::METHOD_LONG_PROPERTY_PLAN } else { 0 })
            | (if property_getter_plan { Self::METHOD_PROPERTY_GETTER_PLAN } else { 0 });
    }

    #[inline(always)]
    pub fn method_fusion_eligible(&self) -> bool {
        self.prop_info & Self::METHOD_FUSION_ELIGIBLE != 0
    }

    #[inline(always)]
    pub fn method_has_long_property_plan(&self) -> bool {
        self.prop_info & Self::METHOD_LONG_PROPERTY_PLAN != 0
    }

    #[inline(always)]
    pub fn method_has_property_getter_plan(&self) -> bool {
        self.prop_info & Self::METHOD_PROPERTY_GETTER_PLAN != 0
    }

    /// String identity retained by a dynamic-callback DoFcall cache entry.
    ///
    /// DoFcall does not otherwise use `class_id` or `prop_info`, so their two
    /// halves can hold the pointer without growing the 16-byte side table.
    #[inline(always)]
    pub fn callback_string(&self) -> *const String {
        let raw = ((self.class_id as u64) << 32) | self.prop_info as u64;
        raw as usize as *const String
    }

    #[inline(always)]
    pub fn set_callback_string(&mut self, key: *const String, func: *const FunctionCommon) {
        let raw = key as usize as u64;
        self.func = func;
        self.class_id = (raw >> 32) as u32;
        self.prop_info = raw as u32;
    }

    #[inline(always)]
    pub fn callback_string_cache_disabled(&self) -> bool {
        self.func == Self::CALLBACK_CACHE_DISABLED
    }

    /// Permanently stop caching a polymorphic dynamic-callback call site.
    /// The caller releases any retained string before entering this state.
    #[inline(always)]
    pub fn disable_callback_string_cache(&mut self) {
        self.func = Self::CALLBACK_CACHE_DISABLED;
        self.class_id = 0;
        self.prop_info = 0;
    }
}
