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

/// Common header shared by all function types.
/// MUST be first field in UserFunction and InternalFunction (#[repr(C)]).
#[repr(C)]
pub struct FunctionCommon {
    pub fn_type: FunctionType,
    pub num_args: u32,
    pub required_num_args: u32,
}

/// User-defined PHP function — contains compiled OpArray.
#[repr(C)]
pub struct UserFunction {
    pub common: FunctionCommon,
    pub op_array: OpArray,
}

/// Handler signature for internal (built-in) functions.
/// Raw pointers because ExecuteData lives on VM stack.
pub type InternalFunctionHandler = fn(
    execute_data: *mut ExecuteData,
    return_value: *mut Value,
    eg: &ExecutorGlobals,
);

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
