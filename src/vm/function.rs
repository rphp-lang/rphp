use std::cell::Cell;
use std::ptr::NonNull;

use crate::compiler::OpArray;
use crate::value::Value;
use crate::runtime::ExecutorGlobals;
use super::frame::ExecuteData;

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
    /// No variadics, no param type hints → skip validation in DoFcall.
    Fast,
    /// Full validation: arity check, type hints, variadic packing.
    Full,
}

/// Return dispatch: Fast skips global/static sync, return type check, try/finally.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ReturnStrategy {
    /// No globals, no statics, no return type, no try/finally, not generator.
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
    /// Whether this function is eligible for hot executor promotion.
    ///
    /// **Single source of truth** — all promotion paths must use this.
    /// `HotStatus::Hot` implies `can_promote_to_hot() == true`.
    ///
    /// A function is hot-eligible if:
    /// - User function (internal functions have opaque handlers)
    /// - FastScalar or Fast call strategy (no variadics, no by-ref)
    /// - Fast return strategy (no globals/statics/try-finally/generator sync)
    /// - No non-trivial param type hints (hot executor skips type validation)
    #[inline]
    pub fn can_promote_to_hot(&self) -> bool {
        self.fn_type == FunctionType::User
            && matches!(self.plan.call, CallStrategy::FastScalar | CallStrategy::Fast)
            && self.plan.ret == ReturnStrategy::Fast
            && (self.sig.param_type_hints.is_empty()
                || self.sig.param_type_hints.iter().all(|h|
                    matches!(h, ParamTypeHint::None | ParamTypeHint::Mixed)))
    }
}

/// User-defined PHP function — contains compiled OpArray.
#[repr(C)]
pub struct UserFunction {
    pub common: FunctionCommon,
    pub op_array: OpArray,
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
