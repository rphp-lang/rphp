pub mod compile;

use crate::value::Value;
use crate::vm::instruction::Instruction;
use crate::vm::function::{
    FunctionCommon, FunctionType, UserFunction, ParamTypeHint,
    InternalFunction, InternalFunctionHandler,
};

/// Compiled function body — equivalent to zend_op_array.
pub struct OpArray {
    pub num_cvs: u32,
    pub num_temps: u32,
    pub instructions: Vec<Instruction>,
    pub literals: Vec<Value>,
    pub try_entries: Vec<compile::TryEntry>,
    /// Per-file strict_types flag, set by `declare(strict_types=1);`
    pub strict_types: bool,
    /// True if this function contains yield — it's a generator
    pub is_generator: bool,
}

impl OpArray {
    pub fn instructions(&self) -> &[Instruction] {
        &self.instructions
    }

    pub fn literals(&self) -> &[Value] {
        &self.literals
    }
}

/// Create a UserFunction wrapping an OpArray (no args — for main script).
pub fn make_user_function(op_array: OpArray) -> UserFunction {
    make_user_function_with_args(op_array, 0)
}

/// Create a UserFunction with the given number of parameters.
pub fn make_user_function_with_args(op_array: OpArray, num_args: u32) -> UserFunction {
    make_user_function_full(op_array, num_args, num_args, false, 0, 0)
}

/// Create a UserFunction with separate total and required arg counts (for default params).
pub fn make_user_function_with_defaults(op_array: OpArray, num_args: u32, required_num_args: u32, is_variadic: bool) -> UserFunction {
    make_user_function_full(op_array, num_args, required_num_args, is_variadic, 0, 0)
}

/// Full constructor with all options.
pub fn make_user_function_full(op_array: OpArray, num_args: u32, required_num_args: u32, is_variadic: bool, variadic_cv_index: u32, ref_args: u64) -> UserFunction {
    UserFunction {
        common: FunctionCommon {
            fn_type: FunctionType::User,
            num_args,
            required_num_args,
            is_variadic,
            variadic_cv_index,
            ref_args,
            this_offset: 0,
            param_type_hints: vec![],
            param_names: vec![],
            return_type_hint: ParamTypeHint::None,
        },
        op_array,
    }
}

/// Extended full constructor with type hints and param names.
pub fn make_user_function_typed(
    op_array: OpArray,
    num_args: u32,
    required_num_args: u32,
    is_variadic: bool,
    variadic_cv_index: u32,
    ref_args: u64,
    param_type_hints: Vec<ParamTypeHint>,
    param_names: Vec<String>,
    return_type_hint: ParamTypeHint,
) -> UserFunction {
    UserFunction {
        common: FunctionCommon {
            fn_type: FunctionType::User,
            num_args,
            required_num_args,
            is_variadic,
            variadic_cv_index,
            ref_args,
            this_offset: 0,
            param_type_hints,
            param_names,
            return_type_hint,
        },
        op_array,
    }
}

/// Create an InternalFunction with the given handler.
pub fn make_internal_function(
    handler: InternalFunctionHandler,
    num_args: u32,
    required_num_args: u32,
    param_names: Vec<String>,
) -> InternalFunction {
    InternalFunction {
        common: FunctionCommon {
            fn_type: FunctionType::Internal,
            num_args,
            required_num_args,
            is_variadic: false,
            variadic_cv_index: 0,
            ref_args: 0,
            this_offset: 0,
            param_type_hints: vec![],
            param_names,
            return_type_hint: ParamTypeHint::None,
        },
        handler,
    }
}

/// Create an InternalFunction for a method (with $this in CV 0).
/// `num_args` includes the hidden $this slot; `this_offset` is set to 1.
pub fn make_internal_method(
    handler: InternalFunctionHandler,
    num_args: u32,
    required_num_args: u32,
    param_names: Vec<String>,
) -> InternalFunction {
    InternalFunction {
        common: FunctionCommon {
            fn_type: FunctionType::Internal,
            num_args,
            required_num_args,
            is_variadic: false,
            variadic_cv_index: 0,
            ref_args: 0,
            this_offset: 1,
            param_type_hints: vec![],
            param_names,
            return_type_hint: ParamTypeHint::None,
        },
        handler,
    }
}

/// Create an InternalFunction with by-ref parameter bitmask.
pub fn make_internal_function_ref(
    handler: InternalFunctionHandler,
    num_args: u32,
    required_num_args: u32,
    ref_args: u64,
    param_names: Vec<String>,
) -> InternalFunction {
    InternalFunction {
        common: FunctionCommon {
            fn_type: FunctionType::Internal,
            num_args,
            required_num_args,
            is_variadic: false,
            variadic_cv_index: 0,
            ref_args,
            this_offset: 0,
            param_type_hints: vec![],
            param_names,
            return_type_hint: ParamTypeHint::None,
        },
        handler,
    }
}

/// Create a variadic InternalFunction.
pub fn make_internal_function_variadic(
    handler: InternalFunctionHandler,
    required_num_args: u32,
    param_names: Vec<String>,
) -> InternalFunction {
    InternalFunction {
        common: FunctionCommon {
            fn_type: FunctionType::Internal,
            num_args: required_num_args,
            required_num_args,
            is_variadic: true,
            variadic_cv_index: required_num_args,
            ref_args: 0,
            this_offset: 0,
            param_type_hints: vec![],
            param_names,
            return_type_hint: ParamTypeHint::None,
        },
        handler,
    }
}
