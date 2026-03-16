pub mod compile;

use crate::value::Value;
use crate::vm::instruction::Instruction;
use crate::vm::function::{
    FunctionCommon, FunctionType, UserFunction,
    InternalFunction, InternalFunctionHandler,
};

/// Compiled function body — equivalent to zend_op_array.
/// For now uses Vec (not raw pointers) since we don't have C extension
/// compatibility yet. Will migrate to raw pointers when needed.
pub struct OpArray {
    pub num_cvs: u32,
    pub num_temps: u32,
    pub instructions: Vec<Instruction>,
    pub literals: Vec<Value>,
    pub try_entries: Vec<compile::TryEntry>,
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
    make_user_function_with_defaults(op_array, num_args, num_args)
}

/// Create a UserFunction with separate total and required arg counts (for default params).
pub fn make_user_function_with_defaults(op_array: OpArray, num_args: u32, required_num_args: u32) -> UserFunction {
    UserFunction {
        common: FunctionCommon {
            fn_type: FunctionType::User,
            num_args,
            required_num_args,
        },
        op_array,
    }
}

/// Create an InternalFunction with the given handler.
pub fn make_internal_function(
    handler: InternalFunctionHandler,
    num_args: u32,
    required_num_args: u32,
) -> InternalFunction {
    InternalFunction {
        common: FunctionCommon {
            fn_type: FunctionType::Internal,
            num_args,
            required_num_args,
        },
        handler,
    }
}
