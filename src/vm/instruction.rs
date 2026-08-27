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

/// DoFcall flag: a source-level `(void)` cast explicitly acknowledges and
/// ignores this return value. This is distinct from an ordinary unused result
/// for PHP 8.5's `#[NoDiscard]` warning contract.
pub const CALL_FLAG_RETURN_EXPLICITLY_IGNORED: u16 = 1 << 11;

/// SendRef/SendVarEx flag: the source expression is the special `$GLOBALS`
/// root. PHP permits reading that table by value but never exposing it through
/// a reference parameter.
pub const SEND_FLAG_GLOBALS: u16 = 1;

/// SendVarEx/SendNamed flag: op1 is a source CV whose ordinary by-value read
/// must report an undefined-variable warning. Runtime signature resolution may
/// instead select a by-reference l-value context, in which case the read stays
/// silent. `result` holds the source variable-name literal.
pub const SEND_FLAG_FETCH_CV_R: u16 = 1 << 1;

/// SendVarEx/SendNamed flag: an annotated by-value CV read is under PHP's `@`
/// reporting mask. A custom handler still runs and observes that mask.
pub const SEND_FLAG_ERROR_SUPPRESS: u16 = 1 << 2;

/// SendVal/SendNamed flag: the source expression produced a value but PHP
/// forbids exposing that expression as a reference (notably a nullsafe chain).
/// Runtime signature resolution raises the ordinary argument Error only when
/// the selected parameter is actually by-reference.
pub const SEND_FLAG_NONREFERENCEABLE: u16 = 1 << 3;

/// SendVarEx/SendNamed flag: op1 is the by-value snapshot taken before a
/// yielding later argument, while `result` names the original source CV.
/// Runtime signature resolution selects the CV alias for a by-reference
/// parameter and the snapshot for an ordinary by-value parameter.
pub const SEND_FLAG_YIELD_SNAPSHOT: u16 = 1 << 4;

/// SendVal/SendVarEx/SendNamed flag: the source is an indirect temporary
/// produced by a call or object construction. PHP lets a hard-reference
/// parameter bind that temporary, but emits E_NOTICE unless the producer
/// returned an actual reference. Direct rvalues use
/// `SEND_FLAG_NONREFERENCEABLE` instead and remain errors.
pub const SEND_FLAG_INDIRECT_TEMPORARY: u16 = 1 << 5;

/// FetchCvR flag: evaluate this read under PHP's `@` reporting mask. Custom
/// handlers still run and observe the suppressed mask.
pub const FETCH_CV_ERROR_SUPPRESS: u16 = 1;
/// Direct increment/decrement executes inside an `@` suppression scope.
pub const INCDEC_ERROR_SUPPRESS: u16 = 1;

/// FetchConst flag: this exact read resolves PHP's deprecated built-in
/// `E_STRICT` constant and must emit its PHP 8.5 use-site diagnostic.
pub const FETCH_CONST_DEPRECATED_E_STRICT: u16 = 1;
/// FetchConst flag: the read is inside `@`; a deprecation handler still runs
/// but observes PHP's fatal-only reporting mask.
pub const FETCH_CONST_ERROR_SUPPRESS: u16 = 1 << 1;

/// A source-level `goto` leaves a try/catch region with finally, while its
/// zero-width target label shares the first executable offset of that region.
pub const JMP_FLAG_TARGET_OUTSIDE_TRY: u16 = 1;
/// The finally-control opcode marks the end of a finally body, not its entry.
pub const JMP_FLAG_FINALLY_END: u16 = 1 << 1;

/// Throw flag: op1 is the unmatched discriminant and runtime must construct
/// PHP's value-specific UnhandledMatchError at this source location.
pub const THROW_FLAG_UNHANDLED_MATCH: u16 = 1;

/// Late-static property flag: the called class lives in the compact frame's
/// embedded scope slot. Wide frames and instance methods use the resolver.
pub const LATE_STATIC_PROP_EMBEDDED_SCOPE: u16 = 1;

/// Fetch/assign static-property flag: op1 is a runtime class expression and
/// may therefore be either a class-name string or an object instance.
pub const STATIC_PROP_DYNAMIC_OWNER: u16 = 1 << 4;

/// Fetch/assign static-property flag: op2 was computed at runtime. Cache hits
/// must therefore verify or re-resolve the selected property name.
pub const STATIC_PROP_DYNAMIC_NAME: u16 = 1 << 5;

/// Fetch static-property flag: the read is an `isset()`/`empty()`-style silent
/// probe. This must not alias the late-static embedded-scope bit.
pub const STATIC_PROP_SILENT: u16 = 1 << 6;
/// Promote the resolved static-property storage slot to a stable reference
/// cell and bind the result CV to that same cell.
pub const STATIC_PROP_REFERENCE_FETCH: u16 = 1 << 7;
/// Rebind the resolved static-property storage slot to the reference supplied
/// in the instruction result operand instead of writing through an old alias.
pub const STATIC_PROP_REFERENCE_BIND: u16 = 1 << 8;
/// Static-property writeback is a read-modify-write operation (increment,
/// compound assignment, dimension mutation or reference access).
pub const STATIC_PROP_INDIRECT_MODIFY: u16 = 1 << 9;

/// CreateClosure flag: PHP's `static function`/`static fn` form cannot bind
/// an object, even when created inside an instance method.
pub const CLOSURE_FLAG_STATIC: u16 = 1;
/// CreateClosure flag: allocate per-object function-static storage only when
/// the compiled anonymous body contains a `static $variable` declaration.
pub const CLOSURE_FLAG_HAS_STATICS: u16 = 1 << 1;
/// The nested closure contains trait-bound `__CLASS__`; its enclosing trait
/// method must be specialized for the final consuming class.
pub const CLOSURE_FLAG_TRAIT_LEXICAL_SCOPE: u16 = 1 << 2;

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

/// Class-constant fetch emitted while materializing a constant expression.
/// PHP resolves an enum case in this context without building the backed-enum
/// lookup table, unlike an ordinary source-level `Enum::Case` read.
pub const CLASS_CONST_CONSTANT_EXPRESSION: u16 = 1 << 4;

/// Relative `::class` fetch used only to materialize the owner of a dynamic
/// static call. An absent class scope uses the member-access diagnostic rather
/// than the standalone pseudo-constant diagnostic.
pub const CLASS_CONST_DYNAMIC_CALL_OWNER: u16 = 1 << 5;

/// `FetchObjR` used only to reach the terminal operand of `isset()`. A null or
/// scalar intermediate produces null without the ordinary read diagnostic.
pub const FETCH_OBJ_SILENT: u16 = 1;

/// An ordinary property read performed under PHP's `@` reporting mask.
/// User handlers still receive the warning with the suppressed error level.
pub const FETCH_OBJ_ERROR_SUPPRESS: u16 = 1 << 1;

/// `FetchObjR` is traversing an intermediate property in a mutable l-value.
/// A null or scalar receiver therefore throws PHP's catchable modification
/// error instead of behaving like an ordinary property read.
pub const FETCH_OBJ_MODIFY: u16 = 1 << 2;

/// `FetchObjR` is the terminal target of property increment/decrement. A null
/// or scalar receiver throws PHP's dedicated inc/dec error.
pub const FETCH_OBJ_INCDEC: u16 = 1 << 3;

/// `FetchObjR` is the deferred read of a compound property assignment. A
/// scalar receiver throws PHP's direct-assignment error before the arithmetic
/// operation is attempted.
pub const FETCH_OBJ_COMPOUND: u16 = 1 << 4;

/// A same-property access compiled inside that property's hook. It addresses
/// backing storage directly even when the hook was entered through an explicit
/// parent call and therefore has no ordinary dispatch guard.
pub const OBJ_PROP_HOOK_BYPASS: u16 = 1 << 5;

/// `FetchObjR` is the terminal source of a by-reference call or foreach. An
/// initialized non-object readonly value cannot enter the temporary reference
/// cell that will be written back after the operation.
pub const FETCH_OBJ_REFERENCE_SOURCE: u16 = 1 << 6;

/// Property read emitted while materializing a constant expression. PHP only
/// permits enum-case receivers in this context; ordinary object receivers are
/// rejected after the dynamic property name has been evaluated and converted.
pub const FETCH_OBJ_CONSTANT_EXPRESSION: u16 = 1 << 7;

/// `AssignObjProp` is materializing a reference binding, which uses PHP's
/// modification diagnostic for a null or scalar receiver.
pub const ASSIGN_OBJ_MODIFY: u16 = 1;
/// Property assignment emitted by PHP 8.5 clone-with. An initialized readonly
/// property may be replaced once by this array entry after `__clone` returns.
pub const ASSIGN_OBJ_CLONE_WITH: u16 = 1 << 1;
/// Instance/static property writeback follows an increment that overflowed a
/// PHP integer. Typed property validation must not weakly coerce that result.
pub const PROPERTY_INCDEC_INCREMENT: u16 = 1 << 2;
/// Instance/static property writeback follows a decrement that underflowed a
/// PHP integer. This shares the assignment-only flag space above.
pub const PROPERTY_INCDEC_DECREMENT: u16 = 1 << 3;
/// An instance-property assignment performed under PHP's `@` reporting mask.
/// Magic setters and user handlers still run and observe the suppressed mask.
pub const ASSIGN_OBJ_ERROR_SUPPRESS: u16 = 1 << 4;
/// A standalone property assignment has no observable expression result. Its
/// TMP/VAR source can be transferred into property storage instead of leaving
/// a compiler-only object handle alive until frame teardown.
pub const ASSIGN_PROP_MOVE_SOURCE: u16 = 1 << 10;
/// `CloneObj` is followed by the PHP 8.5 property-update loop.
pub const CLONE_OBJ_WITH_PROPERTIES: u16 = 1;
/// `BindObjPropRef` rebinds the property to the reference supplied in its
/// result CV. Without this flag the opcode promotes/fetches the property cell
/// and rebinds the result CV instead.
pub const OBJ_PROP_REFERENCE_BIND: u16 = 1 << 1;

/// `FetchDimR` is the terminal probe of `isset($container[$offset])`. Arrays
/// can answer directly; ArrayAccess objects dispatch `offsetExists()` instead
/// of invoking `offsetGet()` and potentially observing or throwing on a miss.
pub const FETCH_DIM_ISSET: u16 = 1;

/// An ordinary dimension read performed under PHP's `@` reporting mask.
/// User handlers still receive the warning with the suppressed error level.
pub const FETCH_DIM_ERROR_SUPPRESS: u16 = 1 << 1;

/// An intermediate dimension read used only to reach the terminal operand of
/// `isset()`/`empty()`. A miss preserves the ordinary empty value but does not
/// emit the standalone read warning.
pub const FETCH_DIM_SILENT: u16 = 1 << 2;

/// `FetchDimR` reads an array element that will be written back by a mutation.
/// Null is treated as an empty array, so a missing key uses the undefined-key
/// diagnostic instead of the ordinary scalar-offset warning.
pub const FETCH_DIM_MUTABLE: u16 = 1 << 3;

/// A dimension read performed while evaluating `empty()`. The fetched value
/// remains available for the truthiness check, but invalid key diagnostics use
/// PHP's shared `isset or empty` wording.
pub const FETCH_DIM_EMPTY: u16 = 1 << 4;

/// `FetchDimR` is reading one element for list/short destructuring. PHP keeps
/// missing-array-key diagnostics, but a null or scalar destructuring source
/// yields null elements without the ordinary scalar-offset warning.
pub const FETCH_DIM_DESTRUCTURE: u16 = 1 << 5;

/// `FetchDimR` evaluates an array dimension used as a runtime-resolved call
/// argument. The pending signature selects ordinary read semantics for a
/// by-value parameter or binds the result CV directly to the array element for
/// a by-reference parameter. `extended_value` stores the one-based public
/// parameter index, or a one-based name-literal index when the named flag is
/// also present.
pub const FETCH_DIM_FUNC_ARG: u16 = 1 << 6;

/// The runtime function-argument selector in `extended_value` is a named
/// argument literal rather than a positional parameter index.
pub const FETCH_DIM_FUNC_ARG_NAMED: u16 = 1 << 7;

/// This function-argument dimension is rooted directly at a source CV. A
/// runtime-selected by-value read of an undefined root reports the ordinary
/// undefined-variable warning before probing its offset; a by-reference
/// parameter instead autovivifies the root silently.
pub const FETCH_DIM_FUNC_ARG_ROOT_CV: u16 = 1 << 8;

/// `FetchDimR` supplies a direct array-dimension expression to a by-reference
/// `foreach`. A string container must normalize the key diagnostics and then
/// reject reference creation before materializing an offset value.
pub const FETCH_DIM_REFERENCE_SOURCE: u16 = 1 << 9;

/// `FetchDynamicVar` reads the symbol-table entry without reporting an
/// undefined-variable diagnostic. Unlike `FETCH_DIM_ISSET`, the fetched value
/// is preserved; this is required by `??=` and by mutations rooted at a
/// runtime-named variable.
pub const FETCH_DYNAMIC_SILENT: u16 = 1 << 1;

/// `FetchDynamicVar` performs an ordinary read while PHP's `@` reporting mask
/// is active. Custom error handlers still run and observe the masked level.
pub const FETCH_DYNAMIC_ERROR_SUPPRESS: u16 = 1 << 2;

/// `Eval` flag: the source-level expression is under PHP's `@` reporting
/// mask. A successful evaluation or catchable parse failure restores the
/// caller mask; a fatal compilation bailout keeps the suppressed mask active
/// through request shutdown.
pub const EVAL_FLAG_ERROR_SUPPRESS: u16 = 1;

/// `FetchDynamicVar` is the read half of one read-modify-write operation. The
/// compiler supplies an owned TMP key, and the fetch replaces it with the
/// converted symbol name before any warning handler can re-enter the caller.
/// The matching writeback therefore targets the symbol selected by the
/// original evaluation without converting or reading a mutable source twice.
pub const FETCH_DYNAMIC_RETAIN_NAME: u16 = 1 << 3;

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
/// NewObj receives a fully materialized source-unpack argument list in op2 and
/// invokes the constructor without an undersized pending call frame.
pub const NEW_FLAG_UNPACKED_ARGUMENTS: u16 = 1 << 4;

/// CallUserFuncArray was emitted for PHP source-level `...` syntax. Its op2 is
/// an internal argument list whose array aliases and Traversable value markers
/// require source-unpack normalization rather than call_user_func_array rules.
pub const CALL_USER_FUNC_ARRAY_SOURCE_UNPACK: u16 = 1;

/// AssignCv replaces the destination CV binding instead of assigning through
/// an existing reference cell. PHP `unset($variable)` uses this to detach the
/// local name while leaving every other alias and the referenced value intact.
pub const ASSIGN_CV_REBIND: u16 = 1;

/// The compiler proved that an unused TMP/VAR source has no consumers after
/// this assignment. Its owner may be transferred into the destination so
/// compiler scratch storage does not extend PHP-visible value lifetime.
pub const ASSIGN_CV_MOVE_SOURCE: u16 = 1 << 1;

/// ReleaseTemps marks an active by-value foreach source that must be retired
/// after return-expression evaluation. Ordinary-object sources execute this
/// marker directly unless a try region requires Return dispatch to defer it
/// through finally; arrays and Traversable sources retain their old lifetime.
pub const RELEASE_TEMPS_ON_RETURN: u16 = 1;

/// AssignDim stores the source l-value's PHP reference cell in the selected
/// element. Ordinary assignments intentionally dereference their source;
/// reference assignments must retain the cell so self-referential arrays and
/// later aliases observe one identity.
pub const ASSIGN_DIM_REFERENCE: u16 = 1;
/// Synthetic parent writeback after nested unset. Scalar parents are validated
/// with unset-specific diagnostics and are never materialized as arrays.
pub const ASSIGN_DIM_UNSET_REBUILD: u16 = 1 << 1;
/// AssignDim is the value-producing write of an assignment expression. If a
/// typed-property reference coerces the stored value, the compiler-generated
/// TMP must expose that coerced value as the expression result.
pub const ASSIGN_DIM_RESULT_VALUE: u16 = 1 << 2;
/// A preceding read of the same compiled key already emitted its scalar-key
/// conversion diagnostic. Compound/incdec writeback must normalize again but
/// must not publish a duplicate warning or deprecation.
pub const ASSIGN_DIM_KEY_ALREADY_NORMALIZED: u16 = 1 << 3;
/// A dimension assignment performed under PHP's `@` reporting mask. String
/// offset conversions and object protocol calls still execute under that mask.
pub const ASSIGN_DIM_ERROR_SUPPRESS: u16 = 1 << 4;
/// UnsetDim addresses the leaf of a multi-dimensional path. String parents use
/// PHP's nested-offset diagnostic rather than the flat string-unset message.
pub const UNSET_DIM_NESTED: u16 = 1;

/// A reference-binding result is stored only in a compiler-generated CV. The
/// handle owns the shared cell for execution lifetime but is not a PHP-visible
/// alias and must not affect reference-wrapper observation.
pub const REFERENCE_RESULT_INTERNAL: u16 = 1;

/// The source of a reference destructuring operation is a temporary
/// expression. A by-value result must emit PHP's non-referenceable notice,
/// while a returned reference remains a valid aliasing source.
pub const REFERENCE_SOURCE_MAY_BE_NONREFERENCEABLE: u16 = 1 << 1;

/// NewObj flag: a literal zero-argument object is assigned to a dead local
/// whose only uses are an immediate bounded span of declared-property reads.
/// A warmed quick loop may project exact scalar defaults without allocating
/// the otherwise-unobservable object owner.
pub const NEW_FLAG_VIRTUAL_DECLARED_READS: u16 = 1 << 3;

/// InitArray flag: at least one compile-time literal string key guarantees
/// general hash storage rather than packed integer storage.
pub const ARRAY_INIT_HASH_HINT: u16 = 1;
/// InitArray flag: op1 is the already-evaluated class half of a dynamic static
/// call. Validate it before evaluating the method expression.
pub const ARRAY_INIT_DYNAMIC_CALL_CLASS: u16 = 1 << 1;
/// Empty compile-time array literal. Non-empty literals are finalized on their
/// last AddArrayElement so construction mutations do not erase provenance.
pub const ARRAY_INIT_IMMUTABLE_LITERAL: u16 = 1 << 2;

/// AddArrayUnpack flag: the array literal is being materialized as a PHP
/// constant expression (for example a constant, parameter default or static
/// local initializer). PHP permits deferred `new` expressions in those
/// contexts, but an object remains invalid as a constant-expression unpack
/// source even when it implements Traversable.
pub const ARRAY_UNPACK_CONSTANT_EXPRESSION: u16 = 1;
/// AddArrayElement flag: preserve the source l-value's PHP reference identity.
pub const ARRAY_ELEMENT_REFERENCE: u16 = 1 << 1;
/// Final element of a fully compile-time array literal. Runtime marks the
/// completed array with Zend's retained immutable-storage provenance.
pub const ARRAY_ELEMENT_FINAL_IMMUTABLE_LITERAL: u16 = 1 << 2;
/// Element stored in an immutable outer literal. A nested immutable array keeps
/// immutable contents but the outer storage becomes its sole source owner.
pub const ARRAY_ELEMENT_IMMUTABLE_CONTAINER: u16 = 1 << 3;
/// ArrayPushOp flag: a by-reference append source is a call result whose
/// referenceability must be diagnosed only after the destination can accept
/// the append. This preserves PHP's overflow-error priority over the later
/// non-variable reference notice.
pub const ARRAY_ELEMENT_DEFER_NONREFERENCEABLE_NOTICE: u16 = 1 << 4;

/// Arithmetic/bitwise opcode flag: the operation is the read phase of a
/// compound assignment. PHP validates commutative binary operands as an
/// unordered pair, but compound assignment converts them in source order so
/// diagnostics from the left operand remain observable before a right-side
/// TypeError. The flag is opcode-local and occupies an otherwise-unused low
/// padding bit without changing the compact instruction layout.
pub const ARITHMETIC_COMPOUND_ASSIGN: u16 = 1;

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

#[cfg(target_pointer_width = "64")]
const _: [(); 16] = [(); std::mem::size_of::<Instruction>()];

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
#[derive(Clone, Copy)]
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
    const DEPRECATED_ENUM_CASE: *const FunctionCommon = 1usize as *const FunctionCommon;
    const GENERIC_CLASS_SCOPE: u32 = 1 << 31;
    const DIRECT_STATIC_TRAIT_ACCESS: u32 = 2;
    const CONSTRUCTOR_HAS_DESTRUCTOR: u32 = 1;

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

    /// Static-property opcodes reserve cache state 2 for a resolved owner that
    /// is itself a trait. Read sites normally use state 1, untyped writes use
    /// state 3, and typed writes use state 1, so the marker keeps ordinary
    /// cache-hit guards byte-for-byte independent from the deprecated path.
    #[inline(always)]
    pub fn static_property_class_id(&self) -> u32 {
        self.class_id
    }

    #[inline(always)]
    pub fn requires_direct_static_trait_deprecation(&self) -> bool {
        self.property_flags() == Self::DIRECT_STATIC_TRAIT_ACCESS
    }

    #[inline(always)]
    pub fn mark_direct_static_trait_access(&mut self) {
        debug_assert_ne!(self.property_flags(), 0);
        self.prop_info =
            (self.prop_info & !Self::PROP_FLAG_MASK) | Self::DIRECT_STATIC_TRAIT_ACCESS;
    }

    /// Enum-case fetches use property flag 2. Their otherwise-idle function
    /// word records whether the cached case needs the cold Deprecated path.
    #[inline(always)]
    pub fn enum_case_requires_deprecated_use_check(&self) -> bool {
        self.property_flags() == 2 && self.func == Self::DEPRECATED_ENUM_CASE
    }

    #[inline]
    pub fn set_enum_case(
        &mut self,
        class_id: u32,
        storage_slot: usize,
        requires_deprecated_use_check: bool,
    ) {
        self.set_property(class_id, storage_slot, 2);
        if requires_deprecated_use_check {
            self.func = Self::DEPRECATED_ENUM_CASE;
        }
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
        if !matches!(self.property_flags(), 1 | Self::DIRECT_STATIC_TRAIT_ACCESS) {
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
        debug_assert!(matches!(
            self.property_flags(),
            1 | Self::DIRECT_STATIC_TRAIT_ACCESS
        ));
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
    pub fn set_constructor(
        &mut self,
        func: *const FunctionCommon,
        class_id: u32,
        has_destructor: bool,
    ) {
        debug_assert!(class_id != 0);
        self.func = func;
        self.class_id = class_id;
        self.prop_info = u32::from(has_destructor) * Self::CONSTRUCTOR_HAS_DESTRUCTOR;
    }

    /// Whether this constructor site must defer automatic destruction until
    /// the original constructor frame returns successfully. NewObj is the
    /// sole owner of this cache meaning, so the packed property word is idle.
    #[inline(always)]
    pub fn constructor_has_destructor(&self) -> bool {
        self.prop_info & Self::CONSTRUCTOR_HAS_DESTRUCTOR != 0
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

    /// Trait-scope methods use the method-cache flags word for the exact
    /// composition class selected by this monomorphic receiver site.
    #[inline(always)]
    pub fn set_method_trait_scope_class_id(&mut self, class_id: u32) {
        // Trait-bound methods must enter a real frame so their hidden TMP is
        // initialized. Retain only generic-contract guards in the low flag
        // bits and use the remaining 27 bits for the ordinary class ID.
        let encoded = if class_id <= (u32::MAX >> 5) {
            class_id << 5
        } else {
            0
        };
        self.prop_info = (self.prop_info
            & (Self::METHOD_GENERIC_CONTRACT | Self::METHOD_LINKED_GENERIC_LONG_CONTRACT))
            | encoded;
    }

    #[inline(always)]
    pub fn method_trait_scope_class_id(&self) -> u32 {
        self.prop_info >> 5
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
    fn static_trait_marker_preserves_the_resolved_class_and_property_slot() {
        let mut cache = InlineCache::empty();
        cache.set_property(7, 3, 3);
        assert_eq!(cache.static_property_class_id(), 7);
        assert!(!cache.requires_direct_static_trait_deprecation());

        cache.mark_direct_static_trait_access();
        assert_eq!(cache.static_property_class_id(), 7);
        assert_eq!(cache.property_slot(), 3);
        assert_eq!(cache.property_flags(), 2);
        assert!(cache.requires_direct_static_trait_deprecation());
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
