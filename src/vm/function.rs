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
    ClassName(std::string::String),
    Nullable(Box<ParamTypeHint>),
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
        }
    }
}

/// Common header shared by all function types.
/// MUST be first field in UserFunction and InternalFunction (#[repr(C)]).
#[repr(C)]
pub struct FunctionCommon {
    pub fn_type: FunctionType,
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

/// Internal (built-in) function — strlen, array_map, etc.
#[repr(C)]
pub struct InternalFunction {
    pub common: FunctionCommon,
    pub handler: InternalFunctionHandler,
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
