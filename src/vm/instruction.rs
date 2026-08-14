use super::function::FunctionCommon;
use super::opcode::OpCode;
use crate::compiler::compile::PropertyDefinition;
use crate::value::ObjectLayout;

/// InitFcall/InitMethodCall flag: every source argument is positional and at
/// least one needs opcodes between Init and its Send. A proven scalar callee may
/// therefore defer its full frame while preserving source evaluation order.
pub const CALL_FLAG_DEFERRED_SCALAR_CANDIDATE: u16 = 1;

/// DoFcall flag: every actually supplied argument was proven to satisfy the
/// statically resolved callee declaration. This includes exact scalar
/// representations and exact declared receiver classes. Arity/hole guards
/// remain in the call protocol; only repeated type validation may be skipped.
pub const CALL_FLAG_EXACT_SCALAR_ARGS: u16 = 1 << 1;

/// InitMethodCall flag: the returned array is assigned to a dead local and
/// immediately consumed by a compiler-proven constant-key Long extraction
/// span. A compatible ObjectArrayFunctionPlan may publish its scalar outputs
/// directly and skip materializing the intermediate PHP array.
pub const CALL_FLAG_OBJECT_ARRAY_CONSUMERS: u16 = 1 << 2;

/// InitFcall flag: this is the outer `array_reduce` of an exact nested
/// map/filter/reduce span whose simple operands may be evaluated by the
/// guarded pure-scalar streaming callback ABI.
pub const CALL_FLAG_CALLBACK_ARRAY_PIPELINE: u16 = 1 << 3;

/// InitFcall flag: this begins an exact staged map/filter/reduce span whose
/// intermediate array CVs provably do not escape their immediate consumers.
pub const CALL_FLAG_STAGED_CALLBACK_ARRAY_PIPELINE: u16 = 1 << 4;

/// InitFcall flag: this begins an exact filter/map/reduce composition whose
/// pure callbacks may run in canonical filter-then-map order while streaming.
pub const CALL_FLAG_FILTER_MAP_CALLBACK_ARRAY_PIPELINE: u16 = 1 << 5;

/// InitFcall flag: exact one-argument json_encode wrapper around an admitted
/// scalar callback pipeline whose final Long can be encoded directly.
pub const CALL_FLAG_CALLBACK_ARRAY_PIPELINE_JSON_SINK: u16 = 1 << 6;
/// Callback-pipeline metadata: filter executes before map.
pub const CALL_FLAG_CALLBACK_ARRAY_PIPELINE_FILTER_FIRST: u16 = 1 << 7;
/// Callback-pipeline metadata: two dead assigned arrays are omitted.
pub const CALL_FLAG_CALLBACK_ARRAY_PIPELINE_STAGED_METADATA: u16 = 1 << 8;

/// InitStaticCall flag: a pseudo-class target belongs to a shared trait body
/// (or another late-bound scope), so it must resolve against the recovered
/// called class instead of reusing the ordinary unqualified call cache.
pub const CALL_FLAG_DYNAMIC_STATIC_SCOPE: u16 = 1 << 9;

/// DoFcall flag: callee execution is under PHP's `@` operator. Runtime installs
/// the suppressed reporting mask for the callee frame and restores it when
/// that frame returns or unwinds.
pub const CALL_FLAG_ERROR_SUPPRESS: u16 = 1 << 10;

/// SendRef/SendVarEx flag: the source expression is the special `$GLOBALS`
/// root. PHP permits reading that table by value but never exposing it through
/// a reference parameter.
pub const SEND_FLAG_GLOBALS: u16 = 1;

/// A source-level `goto` leaves a try/catch region with finally, while its
/// zero-width target label shares the first executable offset of that region.
pub const JMP_FLAG_TARGET_OUTSIDE_TRY: u16 = 1;
/// The finally-control opcode marks the end of a finally body, not its entry.
pub const JMP_FLAG_FINALLY_END: u16 = 1 << 1;

/// Late-static property flag: the called class lives in the compact frame's
/// embedded scope slot. Wide frames and instance methods use the resolver.
pub const LATE_STATIC_PROP_EMBEDDED_SCOPE: u16 = 1;

/// CreateClosure flag: PHP's `static function`/`static fn` form cannot bind
/// an object, even when created inside an instance method.
pub const CLOSURE_FLAG_STATIC: u16 = 1;

/// ClosureUseVar flag: preserve the captured CV's PHP reference cell instead
/// of snapshotting its current value.
pub const CLOSURE_USE_REFERENCE: u16 = 1;

/// Instanceof flag: resolve a pseudo-class name against the active trait or
/// late-static call scope instead of treating it as an ordinary class literal.
pub const INSTANCEOF_DYNAMIC_STATIC_SCOPE: u16 = 1;

/// FetchDynamicClassConst flag: the class owner is a runtime expression rather
/// than a statically named class-like declaration.
pub const CLASS_CONST_DYNAMIC_OWNER: u16 = 1 << 1;

/// Dynamic class-constant flag: the name came from a braced expression.
pub const CLASS_CONST_DYNAMIC_NAME: u16 = 1 << 2;

/// Dynamic class-constant flag: PHP resolved the braced name as a compile-time
/// expression. In particular, a literal `"class"` remains an ordinary lookup
/// while a runtime value equal to `"class"` has PHP's pseudo-constant meaning.
pub const CLASS_CONST_COMPILE_TIME_NAME: u16 = 1 << 3;

/// `FetchObjR` used only to reach the terminal operand of `isset()`. A null or
/// scalar intermediate produces null without the ordinary read diagnostic.
pub const FETCH_OBJ_SILENT: u16 = 1;

/// `FetchDimR` is the terminal probe of `isset($container[$offset])`. Arrays
/// can answer directly; ArrayAccess objects dispatch `offsetExists()` instead
/// of invoking `offsetGet()` and potentially observing or throwing on a miss.
pub const FETCH_DIM_ISSET: u16 = 1;

/// NewObj flag: a constructor-initialized object is assigned once, passed to
/// an immediately scalar-consumed ObjectArray method, and otherwise does not
/// escape. Runtime may represent its declared properties virtually for that
/// span, guarded by the canonical constructor/property caches.
pub const NEW_FLAG_VIRTUAL_OBJECT_ARRAY_PIPELINE: u16 = 1;

/// NewObj flag: resolve `static` (and trait-relative `self`/`parent`) from the
/// runtime called-class scope before autoload, allocation and constructor lookup.
pub const NEW_FLAG_DYNAMIC_STATIC_SCOPE: u16 = 1 << 1;

/// NewObj class name is a runtime expression rather than a literal class name.
pub const NEW_FLAG_DYNAMIC_CLASS_NAME: u16 = 1 << 2;

/// InitArray flag: at least one compile-time literal string key guarantees
/// general hash storage rather than packed integer storage.
pub const ARRAY_INIT_HASH_HINT: u16 = 1;
/// AddArrayElement flag: preserve the source l-value's PHP reference identity.
pub const ARRAY_ELEMENT_REFERENCE: u16 = 1 << 1;

/// Exact scalar representation proven for an instruction result. The fact is
/// stored in otherwise-unused high padding bits so later compiler tiers and a
/// future JIT can consume the same declaration-derived contract without
/// widening the 16-byte instruction.
#[repr(u8)]
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum KnownScalarType {
    Unknown = 0,
    Long = 1,
    Double = 2,
    String = 3,
    Bool = 4,
}

const KNOWN_RESULT_TYPE_SHIFT: u16 = 13;
const KNOWN_RESULT_TYPE_MASK: u16 = 0b111 << KNOWN_RESULT_TYPE_SHIFT;
const METHOD_RETURN_GUARD_SHIFT: u16 = 10;
const METHOD_RETURN_GUARD_MASK: u16 = 0b111 << METHOD_RETURN_GUARD_SHIFT;
const METHOD_LONG_ARGS_GUARD: u16 = 1 << 9;

/// Operand type — where to find the operand
#[repr(u8)]
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum OpType {
    Unused = 0,
    Const = 1, // literal from OpArray.literals
    Tmp = 2,   // temporary variable
    Var = 3,   // VAR (refcounted temporary)
    Cv = 4,    // compiled variable ($a, $b, ...)
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

    #[inline(always)]
    pub fn known_result_type(&self) -> KnownScalarType {
        match (self._pad & KNOWN_RESULT_TYPE_MASK) >> KNOWN_RESULT_TYPE_SHIFT {
            1 => KnownScalarType::Long,
            2 => KnownScalarType::Double,
            3 => KnownScalarType::String,
            4 => KnownScalarType::Bool,
            _ => KnownScalarType::Unknown,
        }
    }

    /// Exact return representation promised by the statically known receiver
    /// contract at an InitMethodCall site. Runtime method dispatch validates
    /// the resolved override once; downstream bytecode can then consume the
    /// result fact without repeating that guard for every operation.
    #[inline(always)]
    pub fn set_method_return_guard_type(&mut self, known: KnownScalarType) {
        self._pad =
            (self._pad & !METHOD_RETURN_GUARD_MASK) | ((known as u16) << METHOD_RETURN_GUARD_SHIFT);
    }

    #[inline(always)]
    pub fn method_return_guard_type(&self) -> KnownScalarType {
        match (self._pad & METHOD_RETURN_GUARD_MASK) >> METHOD_RETURN_GUARD_SHIFT {
            1 => KnownScalarType::Long,
            2 => KnownScalarType::Double,
            3 => KnownScalarType::String,
            4 => KnownScalarType::Bool,
            _ => KnownScalarType::Unknown,
        }
    }

    #[inline(always)]
    pub fn set_method_long_args_guard(&mut self) {
        self._pad |= METHOD_LONG_ARGS_GUARD;
    }

    #[inline(always)]
    pub fn has_method_long_args_guard(&self) -> bool {
        self._pad & METHOD_LONG_ARGS_GUARD != 0
    }

    #[inline]
    pub fn set_known_result_type(&mut self, known: KnownScalarType) {
        self._pad =
            (self._pad & !KNOWN_RESULT_TYPE_MASK) | ((known as u16) << KNOWN_RESULT_TYPE_SHIFT);
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
    const DYNAMIC_PROPERTY_READ_SLOT: usize = (u32::MAX >> 2) as usize;
    pub const TYPED_PROPERTY_COMPLEX: usize = 0;
    pub const TYPED_PROPERTY_INT: usize = 1;
    pub const TYPED_PROPERTY_FLOAT: usize = 2;
    pub const TYPED_PROPERTY_STRING: usize = 3;
    pub const TYPED_PROPERTY_BOOL: usize = 4;
    pub const TYPED_PROPERTY_ARRAY: usize = 5;
    pub const TYPED_PROPERTY_REIFIED: usize = 6;
    const TYPED_PROPERTY_TAG_MASK: usize = 0b111;
    const METHOD_FUSION_ELIGIBLE: u32 = 1;
    const METHOD_LONG_PROPERTY_PLAN: u32 = 2;
    const METHOD_PROPERTY_GETTER_PLAN: u32 = 4;
    const METHOD_GENERIC_CONTRACT: u32 = 8;
    const METHOD_LINKED_GENERIC_LONG_CONTRACT: u32 = 16;
    const CALLBACK_PIPELINE_METADATA_ARMED: u32 = 1 << 31;
    const CALLBACK_CACHE_DISABLED: *const FunctionCommon = 1usize as *const FunctionCommon;
    const GENERIC_CLASS_SCOPE: u32 = 1 << 31;

    pub fn empty() -> Self {
        Self {
            func: std::ptr::null(),
            class_id: 0,
            prop_info: 0,
        }
    }

    /// Declaration ID cached by CheckGenericArgs. Opcode-local cache slots do
    /// not share property/call meanings, so the existing packed word can hold
    /// index+1 without changing InlineCache's 16-byte layout.
    #[inline(always)]
    pub fn generic_declaration(&self) -> Option<u32> {
        (self.prop_info & !Self::GENERIC_CLASS_SCOPE).checked_sub(1)
    }

    #[inline(always)]
    pub fn generic_signature_uses_class_scope(&self) -> bool {
        self.prop_info & Self::GENERIC_CLASS_SCOPE != 0
    }

    #[inline(always)]
    pub fn set_generic_declaration(
        &mut self,
        declaration: u32,
        receiver_class_id: u32,
        callable: *const FunctionCommon,
        uses_class_scope: bool,
    ) {
        self.func = callable;
        self.class_id = receiver_class_id;
        self.prop_info =
            declaration.saturating_add(1) | u32::from(uses_class_scope) * Self::GENERIC_CLASS_SCOPE;
    }

    /// Generic property writes reuse the otherwise-idle function word for a
    /// declaration index. The class, slot and read-safe bit keep their normal
    /// property-cache meanings; absence of the write-safe bit routes Assign
    /// through the substituted type guard.
    #[inline(always)]
    pub fn generic_property_declaration(&self) -> Option<u32> {
        if self.class_id == 0 || self.property_flags() != 1 || self.func.is_null() {
            return None;
        }
        u32::try_from((self.func as usize).checked_sub(1)?).ok()
    }

    #[inline]
    pub fn set_generic_property(&mut self, declaration: u32, class_id: u32, slot: usize) {
        self.set_property(class_id, slot, 1);
        self.func = declaration.saturating_add(1) as usize as *const FunctionCommon;
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
        self.func = std::ptr::null();
        self.class_id = class_id;
        self.prop_info = ((slot as u32) << 2) | flags;
    }

    /// A typed static write may reuse the canonical storage slot but cannot
    /// become write-unconditionally-safe. Keep the stable declaration pointer
    /// in the otherwise-idle function word so warm writes avoid class-table
    /// lookup while still checking the source value.
    #[inline]
    pub fn set_typed_static_property(
        &mut self,
        definition: &PropertyDefinition,
        class_id: u32,
        slot: usize,
    ) {
        let definition_ptr = definition as *const PropertyDefinition;
        debug_assert_eq!(definition_ptr as usize & Self::TYPED_PROPERTY_TAG_MASK, 0);
        self.set_property(class_id, slot, 1);
        let tag = match definition.type_hint {
            super::function::ParamTypeHint::Int => Self::TYPED_PROPERTY_INT,
            super::function::ParamTypeHint::Float => Self::TYPED_PROPERTY_FLOAT,
            super::function::ParamTypeHint::String => Self::TYPED_PROPERTY_STRING,
            super::function::ParamTypeHint::Bool => Self::TYPED_PROPERTY_BOOL,
            super::function::ParamTypeHint::Array => Self::TYPED_PROPERTY_ARRAY,
            _ => Self::TYPED_PROPERTY_COMPLEX,
        };
        self.func = ((definition_ptr as usize) | tag) as *const FunctionCommon;
    }

    /// Typed instance writes use the write-safe bit without the read-safe bit.
    /// Fetch and assignment have separate opcode-local cache entries, so this
    /// state cannot be mistaken for a readable property cache. The tagged
    /// declaration pointer keeps the full guard available without enlarging
    /// the 16-byte cache entry.
    #[inline]
    pub fn set_typed_instance_property(
        &mut self,
        definition: &PropertyDefinition,
        class_id: u32,
        slot: usize,
    ) {
        let definition_ptr = definition as *const PropertyDefinition;
        debug_assert_eq!(definition_ptr as usize & Self::TYPED_PROPERTY_TAG_MASK, 0);
        self.set_property(class_id, slot, 2);
        // Generic-origin contracts must never acquire an exact scalar tag:
        // their substituted bound/reified check remains authoritative even
        // when the erased declaration currently looks like `int` or another
        // scalar. Encoding that distinction on cache fill lets the warmed
        // exact path avoid dereferencing cold declaration metadata.
        let tag = if definition.generic_declaration.is_some() {
            Self::TYPED_PROPERTY_COMPLEX
        } else {
            match definition.type_hint {
                super::function::ParamTypeHint::Int => Self::TYPED_PROPERTY_INT,
                super::function::ParamTypeHint::Float => Self::TYPED_PROPERTY_FLOAT,
                super::function::ParamTypeHint::String => Self::TYPED_PROPERTY_STRING,
                super::function::ParamTypeHint::Bool => Self::TYPED_PROPERTY_BOOL,
                super::function::ParamTypeHint::Array => Self::TYPED_PROPERTY_ARRAY,
                _ => Self::TYPED_PROPERTY_COMPLEX,
            }
        };
        self.func = ((definition_ptr as usize) | tag) as *const FunctionCommon;
    }

    #[inline(always)]
    pub fn typed_instance_property_definition(&self) -> Option<&PropertyDefinition> {
        if self.property_flags() != 2 {
            return None;
        }
        let definition =
            (self.func as usize & !Self::TYPED_PROPERTY_TAG_MASK) as *const PropertyDefinition;
        if definition.is_null() {
            return None;
        }
        // SAFETY: instance cache state 2 with a non-null payload is published
        // only by `set_typed_instance_property`, from an immutable definition
        // in a boxed class that remains stable for the executor lifetime.
        Some(unsafe { &*definition })
    }

    #[inline(always)]
    pub fn typed_instance_property_tag(&self) -> usize {
        debug_assert_eq!(self.property_flags(), 2);
        self.func as usize & Self::TYPED_PROPERTY_TAG_MASK
    }

    #[inline(always)]
    pub fn typed_static_property_definition(&self) -> Option<&PropertyDefinition> {
        if self.property_flags() != 1 {
            return None;
        }
        let definition =
            (self.func as usize & !Self::TYPED_PROPERTY_TAG_MASK) as *const PropertyDefinition;
        if definition.is_null() {
            return None;
        }
        // SAFETY: typed static cache state is published from immutable boxed
        // class metadata. Reified contract tag 6 is handled by its caller and
        // never reaches this accessor as a PropertyDefinition.
        Some(unsafe { &*definition })
    }

    #[inline(always)]
    pub fn typed_static_property_tag(&self) -> usize {
        debug_assert_eq!(self.property_flags(), 1);
        self.func as usize & Self::TYPED_PROPERTY_TAG_MASK
    }

    #[inline]
    pub fn set_reified_static_property(&mut self, contract: *const (), class_id: u32, slot: usize) {
        debug_assert!(!contract.is_null());
        debug_assert_eq!(contract as usize & Self::TYPED_PROPERTY_TAG_MASK, 0);
        self.set_property(class_id, slot, 1);
        self.func = ((contract as usize) | Self::TYPED_PROPERTY_REIFIED) as *const FunctionCommon;
    }

    #[inline(always)]
    pub fn reified_static_property_contract(&self) -> *const () {
        debug_assert_eq!(
            self.typed_static_property_tag(),
            Self::TYPED_PROPERTY_REIFIED
        );
        (self.func as usize & !Self::TYPED_PROPERTY_TAG_MASK) as *const ()
    }

    /// Mark a read site that resolved to the canonical dynamic `stdClass`.
    /// The shared layout pointer guards the receiver shape. An optional small
    /// map position is only a hint and every use validates the current key.
    #[inline]
    pub fn set_dynamic_property_read(
        &mut self,
        property_layout: *const ObjectLayout,
        position: Option<usize>,
    ) {
        debug_assert!(!property_layout.is_null());
        self.set_property(0, position.unwrap_or(Self::DYNAMIC_PROPERTY_READ_SLOT), 1);
        self.func = property_layout.cast();
    }

    #[inline(always)]
    pub fn is_dynamic_property_read(&self) -> bool {
        self.class_id == 0 && self.property_flags() == 1 && !self.func.is_null()
    }

    #[inline(always)]
    pub fn dynamic_property_layout(&self) -> *const ObjectLayout {
        debug_assert!(self.is_dynamic_property_read());
        self.func.cast()
    }

    #[inline(always)]
    pub fn dynamic_property_position(&self) -> Option<usize> {
        debug_assert!(self.is_dynamic_property_read());
        let position = self.property_slot();
        (position != Self::DYNAMIC_PROPERTY_READ_SLOT).then_some(position)
    }

    /// Last validated ordered-entry position for a `FetchDimR` string key.
    /// The array validates both the position and current key before use, so no
    /// array identity or mutation version is required for correctness.
    #[inline(always)]
    pub fn string_array_position(&self) -> Option<usize> {
        (self.prop_info != 0).then(|| (self.prop_info - 1) as usize)
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
        generic_contract: bool,
        linked_generic_long_contract: bool,
    ) {
        self.func = func;
        self.class_id = class_id;
        self.prop_info = (if fusion_eligible {
            Self::METHOD_FUSION_ELIGIBLE
        } else {
            0
        }) | (if long_property_plan {
            Self::METHOD_LONG_PROPERTY_PLAN
        } else {
            0
        }) | (if property_getter_plan {
            Self::METHOD_PROPERTY_GETTER_PLAN
        } else {
            0
        }) | (if generic_contract {
            Self::METHOD_GENERIC_CONTRACT
        } else {
            0
        }) | (if linked_generic_long_contract {
            Self::METHOD_LINKED_GENERIC_LONG_CONTRACT
        } else {
            0
        });
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

    #[inline(always)]
    pub fn method_has_generic_contract(&self) -> bool {
        self.prop_info & Self::METHOD_GENERIC_CONTRACT != 0
    }

    #[inline(always)]
    pub fn method_has_linked_generic_long_contract(&self) -> bool {
        self.prop_info & Self::METHOD_LINKED_GENERIC_LONG_CONTRACT != 0
    }

    /// InitFcall does not otherwise consume `prop_info`; callback-pipeline
    /// sites use its top bit after one full structural validation.
    #[inline(always)]
    pub fn callback_pipeline_metadata_armed(&self) -> bool {
        self.prop_info & Self::CALLBACK_PIPELINE_METADATA_ARMED != 0
    }

    #[inline(always)]
    pub fn arm_callback_pipeline_metadata(&mut self) {
        self.prop_info |= Self::CALLBACK_PIPELINE_METADATA_ARMED;
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

#[cfg(test)]
mod inline_cache_tests {
    use super::InlineCache;
    use crate::compiler::compile::PropertyDefinition;
    use crate::parser::Visibility;
    use crate::value::ObjectLayout;
    use crate::vm::function::ParamTypeHint;

    #[test]
    fn dynamic_property_marker_does_not_alias_a_declared_slot() {
        let mut cache = InlineCache::empty();
        assert!(!cache.is_dynamic_property_read());

        let layout = ObjectLayout::empty();
        cache.set_dynamic_property_read(&layout, Some(2));
        assert!(cache.is_dynamic_property_read());
        assert_eq!(cache.class_id, 0);
        assert_eq!(cache.property_flags(), 1);
        assert_eq!(cache.dynamic_property_layout(), &layout);
        assert_eq!(cache.dynamic_property_position(), Some(2));

        cache.set_property(7, InlineCache::DYNAMIC_PROPERTY_READ_SLOT, 1);
        assert!(!cache.is_dynamic_property_read());
        cache.set_property(0, 3, 1);
        assert!(!cache.is_dynamic_property_read());

        cache.set_dynamic_property_read(&layout, None);
        assert!(cache.is_dynamic_property_read());
        assert_eq!(cache.dynamic_property_position(), None);
    }

    #[test]
    fn generic_property_cache_keeps_class_slot_and_declaration() {
        let mut cache = InlineCache::empty();
        cache.set_generic_property(11, 7, 3);
        assert_eq!(cache.class_id, 7);
        assert_eq!(cache.property_slot(), 3);
        assert_eq!(cache.property_flags(), 1);
        assert_eq!(cache.generic_property_declaration(), Some(11));

        cache.set_property(7, 3, 1);
        assert_eq!(cache.generic_property_declaration(), None);
    }

    #[test]
    fn typed_instance_property_cache_is_distinct_from_read_and_generic_states() {
        let definition = Box::new(PropertyDefinition::declared(
            "number".into(),
            None,
            Visibility::Public,
            "Counter".into(),
            ParamTypeHint::Int,
            false,
            false,
        ));
        let definition_ptr = &*definition as *const PropertyDefinition;
        let mut cache = InlineCache::empty();
        cache.set_typed_instance_property(&definition, 7, 3);

        assert_eq!(cache.class_id, 7);
        assert_eq!(cache.property_slot(), 3);
        assert_eq!(cache.property_flags(), 2);
        assert_eq!(
            cache
                .typed_instance_property_definition()
                .map(|definition| definition as *const PropertyDefinition),
            Some(definition_ptr)
        );
        assert_eq!(
            cache.typed_instance_property_tag(),
            InlineCache::TYPED_PROPERTY_INT
        );
        assert_eq!(cache.generic_property_declaration(), None);
        assert_eq!(std::mem::size_of::<InlineCache>(), 16);

        let mut generic_definition = PropertyDefinition::declared(
            "value".into(),
            None,
            Visibility::Public,
            "Box".into(),
            ParamTypeHint::Int,
            false,
            true,
        );
        generic_definition.generic_declaration = Some(11);
        let generic_definition = Box::new(generic_definition);
        let generic_definition_ptr = &*generic_definition as *const PropertyDefinition;
        cache.set_typed_instance_property(&generic_definition, 9, 4);
        assert_eq!(
            cache
                .typed_instance_property_definition()
                .map(|definition| definition as *const PropertyDefinition),
            Some(generic_definition_ptr)
        );
        assert_eq!(
            cache.typed_instance_property_tag(),
            InlineCache::TYPED_PROPERTY_COMPLEX
        );
    }

    #[test]
    fn method_cache_keeps_generic_contract_proofs_in_free_bits() {
        let mut cache = InlineCache::empty();
        cache.set_method(std::ptr::null(), 7, false, false, false, true, true);
        assert!(cache.method_has_generic_contract());
        assert!(cache.method_has_linked_generic_long_contract());
        assert_eq!(std::mem::size_of::<InlineCache>(), 16);

        cache.set_method(std::ptr::null(), 7, false, false, false, false, false);
        assert!(!cache.method_has_generic_contract());
        assert!(!cache.method_has_linked_generic_long_contract());
    }
}
