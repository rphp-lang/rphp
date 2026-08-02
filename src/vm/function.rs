use std::cell::Cell;
use std::ptr::NonNull;

use crate::compiler::OpArray;
use crate::value::Value;
use crate::runtime::ExecutorGlobals;
use super::frame::ExecuteData;

/// Scalar input consumed by a precompiled long-property method plan.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum LongPlanSource {
    Argument(u8),
    Constant(i64),
}

/// One typed mutation in a frame-free long-property method plan.
#[derive(Debug, Clone, Copy)]
pub enum LongPropertyOp {
    Add { property: u8, rhs: LongPlanSource },
    Sub { property: u8, rhs: LongPlanSource },
    Min { property: u8, candidate: LongPlanSource },
    Max { property: u8, candidate: LongPlanSource },
    Set { property: u8, value: LongPlanSource },
}

/// Inline-cache guard used to resolve a declared public property to its slot.
pub struct LongPlanProperty {
    pub cache_ip: u16,
    pub required_flags: u8,
}

/// Compile-time proof for a small method whose observable work is limited to
/// integer mutations of declared public properties. Runtime evaluation is
/// transactional: every operation completes before any property is committed.
pub struct LongPropertyMethodPlan {
    pub public_args: u8,
    pub properties: Box<[LongPlanProperty]>,
    pub operations: Box<[LongPropertyOp]>,
}

/// Compile-time proof for the exact method body
/// `return $this->declaredPublicProperty`.
///
/// The property name stays in the canonical opcode stream.  The fast path
/// consumes only the FetchObjR inline-cache slot, so class layout and property
/// visibility remain guarded by the same resolution as ordinary execution.
pub struct PropertyGetterMethodPlan {
    pub cache_ip: u16,
}

#[derive(Debug, Clone, Copy)]
pub struct PropertyInitAssignment {
    pub cache_ip: u16,
    pub argument: u8,
}

/// Compile-time proof for a fixed-signature method whose body only copies
/// positional arguments into declared properties of `$this`. New-object sites
/// may use it after the canonical write caches warm, avoiding a constructor
/// ExecuteData frame while preserving argument validation and property layout.
pub struct PropertyInitMethodPlan {
    pub public_args: u8,
    pub assignments: Box<[PropertyInitAssignment]>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum LongRecursiveCondition {
    LessThan,
    LessThanOrEqual,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum LongRecursiveBase {
    Argument,
    Constant(i64),
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum LongRecursiveCombine {
    Add,
    Subtract,
    Multiply,
}

/// A compile-time proof for a pure one-argument binary recurrence:
///
/// `base(n) ? base_value : self(n - first) OP self(n - second)`
///
/// Execution preserves the recursive traversal order but uses compact typed
/// activations rather than a complete PHP ExecuteData frame for every node.
pub struct BinaryLongRecursionPlan {
    pub condition: LongRecursiveCondition,
    pub threshold: i64,
    pub base: LongRecursiveBase,
    pub first_delta: i64,
    pub second_delta: i64,
    pub combine: LongRecursiveCombine,
    /// Present for `$this->method()` recursion. Runtime verifies that virtual
    /// dispatch on the current receiver still resolves to this exact method.
    pub method_name: Option<Box<str>>,
}

/// One scalar input to a frame-elidable integer function plan.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ScalarLongSource {
    /// External input whose binding belongs to the execution adapter. Function
    /// bodies bind it to a public argument; quick regions bind it to a CV slot.
    Input(u16),
    Constant(i64),
    Temporary(u8),
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ScalarLongOpKind {
    Add,
    Subtract,
    Multiply,
    Modulo,
    BitwiseXor,
}

/// One straight-line operation in a pure integer function plan. The result is
/// stored at this operation's position and may be consumed by later steps.
#[derive(Debug, Clone, Copy)]
pub struct ScalarLongOp {
    pub kind: ScalarLongOpKind,
    pub lhs: ScalarLongSource,
    pub rhs: ScalarLongSource,
}

/// Shared typed scalar program used by call arguments, functions and methods.
/// External inputs are described by `ScalarLongSource`; arithmetic temporaries
/// are local to the program and outputs are resolved after all operations.
#[derive(Debug, Clone)]
pub struct ScalarLongProgram<Operation = ScalarLongOp, const OUTPUT_CAPACITY: usize = 8> {
    pub operations: Box<[Operation]>,
    /// Inline output storage avoids a second allocation and pointer chase in
    /// hot calls. Function bodies instantiate one slot, while argument plans
    /// use the guarded scalar ABI capacity of eight.
    pub outputs: [ScalarLongSource; OUTPUT_CAPACITY],
    pub output_count: u8,
}

/// One operand in a pure integer branch predicate. Bitwise masking is kept in
/// the predicate instead of widening the arithmetic IR: it has no observable
/// state and is a common way to express flags and parity checks.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ScalarLongConditionOperand {
    Source(ScalarLongSource),
    BitwiseAnd {
        lhs: ScalarLongSource,
        rhs: ScalarLongSource,
    },
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ScalarLongConditionKind {
    Equal,
    NotEqual,
    LessThan,
    LessThanOrEqual,
}

/// A side-effect-free Long predicate evaluated after the shared arithmetic
/// program. Both return arms are themselves scalar sources in that program.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct ScalarLongSelect {
    pub kind: ScalarLongConditionKind,
    pub lhs: ScalarLongConditionOperand,
    pub rhs: ScalarLongConditionOperand,
    /// Operations before the branch. Predicate temporaries may only refer to
    /// this prefix.
    pub shared_operation_count: u8,
    /// Operations belonging to the true edge. False-edge operations follow
    /// them in the shared program, allowing runtime to execute only one arm.
    pub when_true_operation_count: u8,
    pub when_true: ScalarLongSource,
    pub when_false: ScalarLongSource,
}

/// Compile-time proof that a fixed-signature user function or method consists
/// solely of a small, side-effect-free integer expression and a return.
///
/// Runtime guards require raw Long arguments and checked arithmetic. A failed
/// guard leaves the ordinary ExecuteData call completely untouched, so PHP's
/// generic numeric and error semantics remain the canonical fallback.
pub struct ScalarLongFunctionPlan {
    pub public_args: u8,
    pub program: ScalarLongProgram<ScalarLongOp, 1>,
    /// Present for a pure `if`/guard-clause body with one scalar return on each
    /// edge. `None` preserves the compact straight-line leaf representation.
    pub select: Option<ScalarLongSelect>,
}

/// Scalar source in a small guarded object-reading program. Slot indices use
/// the callee's already-resolved CV/TMP layout, so the executor needs no PHP
/// frame merely to address local integer values.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ObjectLongSource {
    Slot(u16),
    Constant(i64),
}

/// Object input whose declared property is read by an object-long program.
/// Only the method receiver and fixed positional arguments are admitted; a
/// computed object would require executing the canonical body first.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ObjectLongObjectSource {
    Receiver,
    Argument(u8),
}

/// One canonical instruction in a frame-free, read-only object/Long body.
/// The representation intentionally preserves instruction indices so guarded
/// forward branches need no separate CFG or target rewriting.
#[derive(Debug, Clone, Copy)]
pub enum ObjectLongOp {
    Noop,
    Assign {
        destination: u16,
        source: ObjectLongSource,
    },
    FetchProperty {
        object: ObjectLongObjectSource,
        cache_ip: u16,
        destination: u16,
    },
    Arithmetic {
        kind: ScalarLongOpKind,
        lhs: ObjectLongSource,
        rhs: ObjectLongSource,
        destination: u16,
    },
    Compare {
        kind: ScalarLongConditionKind,
        lhs: ObjectLongSource,
        rhs: ObjectLongSource,
        destination: u16,
    },
    StringLiteralBranch {
        argument: u8,
        literal: u16,
        jump_when_equal: bool,
        target: u16,
    },
    IntDiv {
        lhs: ObjectLongSource,
        rhs: ObjectLongSource,
        destination: u16,
    },
    JumpIfFalse {
        condition: ObjectLongSource,
        target: u16,
    },
    JumpIfTrue {
        condition: ObjectLongSource,
        target: u16,
    },
    Jump {
        target: u16,
    },
    Return {
        value: ObjectLongSource,
    },
    /// Canonical implicit `return null` (or another deliberately unsupported
    /// edge). Reaching it side-exits before any observable state was changed.
    Bail,
}

#[derive(Debug, Clone, Copy)]
pub struct ObjectLongIntDivArm {
    pub multiplier: i64,
    pub divisor: i64,
}

#[derive(Debug, Clone, Copy)]
pub struct ObjectLongStringIntDivCase {
    pub literal: u16,
    pub arm: ObjectLongIntDivArm,
}

/// Compact lowering for a common policy/router shape: select an integer
/// multiply+intdiv return arm by comparing one immutable String argument with
/// a short list of literals. The canonical operation program remains the
/// fallback and future JIT input.
pub struct ObjectLongStringIntDivSelect {
    pub string_argument: u8,
    pub input: ObjectLongSource,
    pub cases: Box<[ObjectLongStringIntDivCase]>,
    pub default_arm: ObjectLongIntDivArm,
}

/// Compile-time proof for a fixed-signature method that only reads declared
/// object properties and performs checked integer work. Property inline
/// caches remain authoritative at runtime, preserving class layout and PHP
/// visibility semantics across polymorphic calls.
pub struct ObjectLongFunctionPlan {
    pub public_args: u8,
    pub long_argument_mask: u8,
    pub object_argument_mask: u8,
    pub string_argument_mask: u8,
    pub slot_count: u16,
    pub operations: Box<[ObjectLongOp]>,
    pub string_intdiv_select: Option<Box<ObjectLongStringIntDivSelect>>,
}

/// Scalar/value input in a small read-only application method. Property
/// sources deliberately retain the canonical FetchObjR cache position: a
/// runtime class/layout mismatch therefore side-exits before the plan creates
/// its result array.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ObjectArraySource {
    Receiver,
    Argument(u8),
    LongSlot(u16),
    Literal(u16),
    Property {
        object: ObjectLongObjectSource,
        cache_ip: u16,
    },
}

/// One nested monomorphic call whose target is itself proven by an
/// ObjectLongFunctionPlan. The owning method's inline cache remains the
/// dispatch authority; no class or method identity is baked into this IR.
pub struct ObjectArrayLongCall {
    pub cache_ip: u16,
    pub receiver: ObjectArraySource,
    pub arguments: Box<[ObjectArraySource]>,
    pub destination: u16,
}

pub enum ObjectArrayLongOp {
    Assign {
        destination: u16,
        source: ObjectArraySource,
    },
    Arithmetic {
        kind: ScalarLongOpKind,
        lhs: ObjectArraySource,
        rhs: ObjectArraySource,
        destination: u16,
    },
    IntDiv {
        lhs: ObjectArraySource,
        rhs: ObjectArraySource,
        destination: u16,
    },
    Call(ObjectArrayLongCall),
}

#[derive(Debug, Clone, Copy)]
pub struct ObjectArrayEntry {
    pub key_literal: u16,
    pub value: ObjectArraySource,
}

/// Compile-time proof for a read-only object method that composes guarded
/// Long-returning methods and checked integer work into a small associative
/// array. Evaluation is transactional: all guards and arithmetic complete
/// before the array becomes observable, so failure can replay canonical PHP.
pub struct ObjectArrayFunctionPlan {
    pub public_args: u8,
    pub slot_count: u16,
    pub operations: Box<[ObjectArrayLongOp]>,
    pub entries: Box<[ObjectArrayEntry]>,
}

/// Branch metadata for a pure function that selects one of two immutable
/// string values from a guarded Long predicate.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct ScalarStringSelect {
    pub kind: ScalarLongConditionKind,
    pub lhs: ScalarLongConditionOperand,
    pub rhs: ScalarLongConditionOperand,
}

/// Compile-time proof that a fixed-signature function returns an immutable
/// string selected by a pure Long expression. The strings are owned by the
/// function plan, so typed consumers may borrow them without constructing a
/// PHP `Value` or touching a reference count.
pub struct ScalarStringFunctionPlan {
    pub public_args: u8,
    pub operations: Box<[ScalarLongOp]>,
    pub select: Option<ScalarStringSelect>,
    pub when_true: Box<str>,
    pub when_false: Box<str>,
}

/// Dispatch identity required by a typed scalar call. Cache positions are
/// relative to the owning canonical bytecode. Method calls additionally bind
/// the cache identity to the current class of an object CV.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ScalarLongCallGuard {
    FunctionCache { cache_ip: u32 },
    MethodCache { cache_ip: u32, receiver_slot: u16 },
}

impl ScalarLongCallGuard {
    #[inline(always)]
    pub const fn cache_ip(self) -> usize {
        match self {
            Self::FunctionCache { cache_ip }
            | Self::MethodCache { cache_ip, .. } => cache_ip as usize,
        }
    }
}

/// Guarded scalar call embedded in a typed program. The dispatch guard remains
/// the source of truth for runtime function or method identity.
pub struct ScalarLongCall {
    pub guard: ScalarLongCallGuard,
    pub arguments: Box<[ScalarLongSource]>,
}

/// String result produced by an earlier typed operation. String values remain
/// separate from Long temporaries even though both use the operation index as
/// their compact SSA identity.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ScalarStringSource {
    Temporary(u8),
}

/// One instruction in a pure scalar body that composes direct user calls with
/// checked integer arithmetic.
pub enum ComposedScalarLongOp {
    Arithmetic(ScalarLongOp),
    Call(ScalarLongCall),
}

/// Typed composed operations that can keep an immutable String borrowed until
/// it is consumed by a scalar operation. Kept separate from the two-variant
/// Long enum so adding heap-capable types cannot perturb the established Long
/// executor's inner dispatch.
pub enum ComposedTypedLongOp {
    Arithmetic(ScalarLongOp),
    Call(ScalarLongCall),
    StringCall(ScalarLongCall),
    StringConcatLiteral {
        value: ScalarStringSource,
        literal_len: u32,
    },
    StringLength(ScalarStringSource),
}

/// Compile-time proof for a straight-line scalar body containing pure direct
/// function calls, for example `return add1($a) + double($b)`.
pub struct ComposedScalarLongFunctionPlan {
    pub public_args: u8,
    /// Public arguments consumed as raw Long values by arithmetic or nested
    /// scalar-call arguments.
    pub long_argument_mask: u8,
    /// Public arguments used only as guarded object receivers. These inputs
    /// never enter `ScalarLongSource` arithmetic.
    pub object_argument_mask: u8,
    pub program: ScalarLongProgram<ComposedScalarLongOp, 1>,
}

pub struct ComposedTypedLongFunctionPlan {
    pub public_args: u8,
    pub long_argument_mask: u8,
    pub object_argument_mask: u8,
    pub program: ScalarLongProgram<ComposedTypedLongOp, 1>,
}

/// Function type discriminant
#[repr(u8)]
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum FunctionType {
    Undef = 0,
    User = 1,
    Internal = 2,
}

/// Runtime representation of a parameter type hint.
#[derive(Debug, Clone, PartialEq)]
pub enum ParamTypeHint {
    None,
    Int,
    Float,
    String,
    Bool,
    Array,
    Callable,
    Void,
    Mixed,
    Never,
    ClassName(std::string::String),
    Nullable(Box<ParamTypeHint>),
    Union(Vec<ParamTypeHint>),
}

impl ParamTypeHint {
    /// Human-readable name for error messages.
    pub fn display_name(&self) -> std::string::String {
        match self {
            ParamTypeHint::None => "mixed".to_string(),
            ParamTypeHint::Int => "int".to_string(),
            ParamTypeHint::Float => "float".to_string(),
            ParamTypeHint::String => "string".to_string(),
            ParamTypeHint::Bool => "bool".to_string(),
            ParamTypeHint::Array => "array".to_string(),
            ParamTypeHint::Callable => "callable".to_string(),
            ParamTypeHint::ClassName(name) => name.clone(),
            ParamTypeHint::Nullable(inner) => format!("?{}", inner.display_name()),
            ParamTypeHint::Void => "void".to_string(),
            ParamTypeHint::Mixed => "mixed".to_string(),
            ParamTypeHint::Never => "never".to_string(),
            ParamTypeHint::Union(parts) => {
                parts.iter().map(|p| p.display_name()).collect::<Vec<_>>().join("|")
            }
        }
    }
}

/// DoFcall dispatch: controls how much validation happens at call boundary.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum CallStrategy {
    /// Tightest path: fixed arity, no by-ref, no variadics, no type hints,
    /// no globals/statics, no generator, no try/finally, no return type.
    /// Enables: inlined scalar SendVal, minimal DoFcall, direct Return.
    /// Typical profile: `fib($n)`, `add($a, $b)`.
    FastScalar,
    /// No variadics and only compact parameter hints. The call boundary
    /// validates scalar tags, arrays and declared object classes without
    /// entering the canonical diagnostic/variadic path.
    Fast,
    /// Full validation: arity check, type hints, variadic packing.
    Full,
    /// Fixed-arity scalar path whose public parameters are all declared
    /// `int`. The boundary validates them once; direct Long plans satisfy the
    /// same contract through their existing argument guards.
    ///
    /// Kept after the original variants so adding the typed ABI does not
    /// change their discriminants or perturb the established untyped paths.
    FastTypedScalar,
}

impl CallStrategy {
    /// Whether compiler-proven Long plans can satisfy this ABI entirely with
    /// their existing input and checked-arithmetic guards.
    #[inline(always)]
    pub fn supports_scalar_long_plan(self) -> bool {
        matches!(self, Self::FastScalar | Self::FastTypedScalar)
    }

    /// User-call strategies understood by the compact scalar boundary.
    #[inline(always)]
    pub fn is_compact_user_call(self) -> bool {
        matches!(self, Self::FastScalar | Self::Fast | Self::FastTypedScalar)
    }
}

/// Return dispatch: Fast skips global/static sync and try/finally, while
/// validating simple scalar return hints inline.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ReturnStrategy {
    /// No globals/statics/try-finally/generator and at most a scalar type hint.
    Fast,
    /// Full return: sync globals/statics, check return type, handle finally.
    Full,
}

/// Cleanup dispatch: controls whether frame slot scan is needed after return.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum CleanupMode {
    /// Function only produces scalar values → skip heap slot scan.
    SkipScan,
    /// May produce heap values → scan all slots for drop.
    ScanAll,
}

/// Parameter metadata — types, names, arity, reference passing.
/// Everything needed for argument validation and named-arg resolution.
pub struct SignatureInfo {
    /// Total number of CV slots used by this function's parameters.
    /// For internal methods: includes hidden $this (e.g. __construct($msg) = 2).
    /// For user functions: only declared params (op_array.num_cvs handles $this separately).
    pub num_args: u32,
    /// Minimum number of explicit (public) arguments required.
    pub required_num_args: u32,
    pub is_variadic: bool,
    /// CV index where variadic args array is stored (only valid when is_variadic=true)
    pub variadic_cv_index: u32,
    /// Bitmask: bit N = 1 means parameter N is pass-by-reference.
    /// Supports up to 64 parameters.
    pub ref_args: u64,
    /// Number of hidden CV slots before explicit args (0 for functions, 1 for methods with $this).
    /// DoFcall uses `num_args - this_offset` for public arity check.
    pub this_offset: u32,
    /// Per-parameter type hints (indexed by public param position, 0-based).
    /// Empty vec = no type hints declared.
    pub param_type_hints: Vec<ParamTypeHint>,
    /// Per-parameter names (indexed by public param position, 0-based).
    /// Used for named argument resolution.
    pub param_names: Vec<std::string::String>,
    /// Declared return type hint (None = no return type declared).
    pub return_type_hint: ParamTypeHint,
}

impl SignatureInfo {
    /// Number of public (user-visible) parameters, excluding hidden $this.
    #[inline]
    pub fn public_arity(&self) -> u32 {
        self.num_args - self.this_offset
    }

    /// Whether public parameter at 0-based index `idx` is pass-by-reference.
    #[inline]
    pub fn is_param_by_ref(&self, idx: u32) -> bool {
        idx < 64 && (self.ref_args & (1u64 << idx)) != 0
    }

    /// CV index for a public parameter at 0-based index `idx`.
    #[inline]
    pub fn param_cv_index(&self, idx: u32) -> u32 {
        idx + self.this_offset
    }

    /// Scalar ABI selected solely from declarations. Structural eligibility
    /// (arity, refs, globals, generators, try/finally) is checked by callers.
    #[inline]
    pub fn declared_scalar_call_strategy(&self) -> Option<CallStrategy> {
        let untyped_params = self
            .param_type_hints
            .iter()
            .all(|hint| matches!(hint, ParamTypeHint::None | ParamTypeHint::Mixed));
        let untyped_return = matches!(
            self.return_type_hint,
            ParamTypeHint::None | ParamTypeHint::Mixed
        );
        if untyped_params && untyped_return {
            return Some(CallStrategy::FastScalar);
        }

        let exact_long_params = !self.param_type_hints.is_empty()
            && self
                .param_type_hints
                .iter()
                .all(|hint| matches!(hint, ParamTypeHint::Int));
        let exact_long_return = matches!(
            self.return_type_hint,
            ParamTypeHint::None | ParamTypeHint::Mixed | ParamTypeHint::Int
        );
        (exact_long_params && exact_long_return).then_some(CallStrategy::FastTypedScalar)
    }
}

/// Frame geometry — precomputed slot counts for push_call_frame.
pub struct FrameLayout {
    /// Precomputed frame CV count (User: op_array.num_cvs, Internal: num_args + variadic).
    pub num_cvs: u32,
    /// Precomputed frame TMP count (User: op_array.num_temps, Internal: 0).
    pub num_temps: u32,
    /// Precomputed total slot count for a normal in-arity call frame.
    /// Equals CALL_FRAME_SLOTS + num_cvs + num_temps.
    pub total_slots: u32,
}

/// Precomputed call strategy — controls fast/slow path dispatch.
/// Set once at construction, avoids repeated runtime checks on hot path.
pub struct CallPlan {
    pub call: CallStrategy,
    pub ret: ReturnStrategy,
    pub cleanup: CleanupMode,
    /// `$this` may be copied into a nested method frame without incrementing
    /// its Rc. The caller owns the object for the entire synchronous call and
    /// the method has no direct `return $this` path.
    pub borrow_this: bool,
}

/// Hotness state for function-level tiering.
/// Transitions: Cold → Hot (when call_count crosses threshold).
/// Unplannable functions (generators, try/finally, variadics) stay Cold permanently.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum HotStatus {
    /// Not yet hot — use baseline interpreter.
    Cold,
    /// Crossed hotness threshold — eligible for hot executor.
    /// Future: will carry an ExecPlan reference.
    Hot,
}

/// Threshold for function call count before promoting to Hot.
/// Tuned for fib-like workloads: 8 calls is enough to identify recursive hot functions
/// without adding overhead to one-shot functions.
pub const FUNC_HOT_THRESHOLD: u32 = 8;

/// Common header shared by all function types.
/// MUST be first field in UserFunction and InternalFunction (#[repr(C)]).
///
/// Four concerns, cleanly separated:
/// - `sig`:   parameter metadata (arity, types, names, ref passing)
/// - `frame`: precomputed frame geometry (slot counts)
/// - `plan`:  precomputed call strategy (fast/slow path flags)
/// - `hot_status` + `call_count`: function-level hotness for tiered execution
#[repr(C)]
pub struct FunctionCommon {
    pub fn_type: FunctionType,
    pub sig: SignatureInfo,
    pub frame: FrameLayout,
    pub plan: CallPlan,
    /// Number of times this function has been called. Saturates at u32::MAX.
    /// Cell because FunctionCommon is shared via raw pointer — single-threaded VM.
    pub call_count: Cell<u32>,
    /// Current hotness tier. Transitions Cold → Hot after call_count >= FUNC_HOT_THRESHOLD.
    pub hot_status: Cell<HotStatus>,
}

impl FunctionCommon {
    /// Whether this signature can enter a guarded frame-free Long plan.
    ///
    /// `Fast` remains the canonical strategy for a return-only `: int`
    /// declaration so fallback still validates that return. A proven Long
    /// plan may nevertheless satisfy the same contract when every argument
    /// is either untyped/mixed or declared int and runtime Long guards pass.
    #[inline(always)]
    pub fn supports_scalar_long_plan(&self) -> bool {
        if self.plan.call.supports_scalar_long_plan() {
            return true;
        }
        self.plan.call == CallStrategy::Fast
            && self.sig.ref_args == 0
            && self.sig.param_type_hints.iter().all(|hint| {
                matches!(hint, ParamTypeHint::None | ParamTypeHint::Mixed | ParamTypeHint::Int)
            })
            && matches!(
                self.sig.return_type_hint,
                ParamTypeHint::None | ParamTypeHint::Mixed | ParamTypeHint::Int
            )
    }

    /// Whether this function is eligible for hot executor promotion.
    ///
    /// **Single source of truth** — all promotion paths must use this.
    /// `HotStatus::Hot` implies `can_promote_to_hot() == true`.
    ///
    /// A function is hot-eligible if:
    /// - User function (internal functions have opaque handlers)
    /// - Compact call strategy (no variadics, no by-ref)
    /// - Fast return strategy (no globals/statics/try-finally/generator sync)
    /// - Only scalar param hints that the compact call boundary can validate
    #[inline]
    pub fn can_promote_to_hot(&self) -> bool {
        self.fn_type == FunctionType::User
            && self.plan.call.is_compact_user_call()
            && !self.sig.is_variadic
            && self.sig.ref_args == 0
            && self.plan.ret == ReturnStrategy::Fast
            && (self.sig.param_type_hints.is_empty()
                || self.sig.param_type_hints.iter().all(|h|
                    matches!(h,
                        ParamTypeHint::None
                            | ParamTypeHint::Int
                            | ParamTypeHint::Float
                            | ParamTypeHint::String
                            | ParamTypeHint::Bool
                            | ParamTypeHint::Mixed
                    )))
    }
}

/// User-defined PHP function — contains compiled OpArray.
#[repr(C)]
pub struct UserFunction {
    pub common: FunctionCommon,
    pub op_array: OpArray,
    pub long_property_plan: Option<Box<LongPropertyMethodPlan>>,
    pub property_getter_plan: Option<PropertyGetterMethodPlan>,
    pub property_init_plan: Option<Box<PropertyInitMethodPlan>>,
    pub binary_long_recursion_plan: Option<BinaryLongRecursionPlan>,
    pub scalar_long_plan: Option<Box<ScalarLongFunctionPlan>>,
    pub object_long_plan: Option<Box<ObjectLongFunctionPlan>>,
    pub object_array_plan: Option<Box<ObjectArrayFunctionPlan>>,
    pub scalar_string_plan: Option<Box<ScalarStringFunctionPlan>>,
    pub composed_scalar_long_plan: Option<Box<ComposedScalarLongFunctionPlan>>,
    pub composed_typed_long_plan: Option<Box<ComposedTypedLongFunctionPlan>>,
    /// Last one- or two-class argument tuple that satisfied this declaration.
    /// Stable class IDs make repeated monomorphic DTO/service calls a single
    /// integer guard while new subclasses retain the canonical hierarchy check.
    pub compact_class_guard: Cell<u64>,
    /// Public by-value parameters that may borrow an immutable heap Value from
    /// their synchronous caller. Indexed by public parameter position.
    pub borrowable_heap_args: u64,
}

/// Handler signature for internal (built-in) functions.
/// Raw pointers because ExecuteData lives on VM stack.
/// eg is &mut to allow VM re-entry (e.g. array_map calling callbacks).
/// Returns Result to propagate fatal errors through DoFcall.
pub type InternalFunctionHandler = fn(
    execute_data: *mut ExecuteData,
    return_value: *mut Value,
    eg: &mut ExecutorGlobals,
) -> Result<(), crate::vm::execute::VmError>;

/// Frame-free ABI for pure, read-only built-ins.
///
/// A direct handler borrows positional arguments, returns an owned PHP value,
/// and cannot access ExecutorGlobals. Built-ins that mutate arguments, retain
/// borrows, depend on caller scope, or re-enter the VM must keep the ordinary
/// ExecuteData ABI.
pub type DirectInternalFunctionHandler =
    fn(&[Value]) -> Result<Value, crate::vm::execute::VmError>;

/// Internal (built-in) function — strlen, array_map, etc.
#[repr(C)]
pub struct InternalFunction {
    pub common: FunctionCommon,
    pub handler: InternalFunctionHandler,
    pub direct_handler: Option<DirectInternalFunctionHandler>,
}

/// Safe wrapper over function pointer — dispatch via fn_type().
/// Never stores owned data, just a NonNull pointer to the common header.
pub struct Function {
    ptr: NonNull<FunctionCommon>,
}

impl Function {
    /// Construct from raw pointer to FunctionCommon header.
    /// SAFETY: ptr must point to a valid FunctionCommon with correct fn_type.
    #[inline]
    pub unsafe fn from_common_ptr(ptr: *const FunctionCommon) -> Self {
        Self {
            ptr: NonNull::new_unchecked(ptr as *mut FunctionCommon),
        }
    }

    #[inline]
    pub fn common(&self) -> &FunctionCommon {
        unsafe { self.ptr.as_ref() }
    }

    #[inline]
    pub fn fn_type(&self) -> FunctionType {
        self.common().fn_type
    }

    /// Return the underlying raw pointer.
    #[inline]
    pub fn as_common_ptr(&self) -> *const FunctionCommon {
        self.ptr.as_ptr() as *const FunctionCommon
    }

    /// SAFETY: caller must verify fn_type() == User
    #[inline]
    pub unsafe fn as_user(&self) -> &UserFunction {
        debug_assert!(self.fn_type() == FunctionType::User);
        &*(self.ptr.as_ptr() as *const UserFunction)
    }

    /// SAFETY: caller must verify fn_type() == Internal
    #[inline]
    pub unsafe fn as_internal(&self) -> &InternalFunction {
        debug_assert!(self.fn_type() == FunctionType::Internal);
        &*(self.ptr.as_ptr() as *const InternalFunction)
    }

    /// Safe dispatch — pattern match on fn_type.
    pub fn dispatch<R>(
        &self,
        user: impl FnOnce(&UserFunction) -> R,
        internal: impl FnOnce(&InternalFunction) -> R,
    ) -> R {
        match self.fn_type() {
            FunctionType::User => user(unsafe { self.as_user() }),
            FunctionType::Internal => internal(unsafe { self.as_internal() }),
            FunctionType::Undef => panic!("dispatch on undefined function"),
        }
    }
}
