pub mod compile;

use crate::value::Value;
use crate::vm::instruction::{Instruction, InlineCache};
use crate::vm::function::{
    FunctionCommon, FunctionType, UserFunction, ParamTypeHint,
    InternalFunction, InternalFunctionHandler,
    SignatureInfo, FrameLayout, CallPlan,
    CallStrategy, ReturnStrategy, CleanupMode,
};
use crate::vm::opcode::OpCode;

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
    /// CVs bound to global variables via explicit `global $x;`: (cv_index, variable_name)
    pub global_vars: Vec<(u32, String)>,
    /// CVs bound to static variables: (cv_index, variable_name)
    pub static_vars: Vec<(u32, String)>,
    /// Function name (for static variable storage key)
    pub name: String,
    /// Main script scope CVs — all top-level variables synced to eg.globals before function calls.
    /// Empty for non-main-script op_arrays.
    pub main_scope_vars: Vec<(u32, String)>,
    /// All CVs in this op_array: (cv_index, variable_name).
    /// Used by include to share the caller's full local scope.
    pub all_cvs: Vec<(u32, String)>,
    /// Inline cache side table — one entry per instruction.
    /// Only InitFcall/InitMethodCall/InitStaticCall use their entries.
    pub cache: Vec<InlineCache>,
}

impl OpArray {
    pub fn instructions(&self) -> &[Instruction] {
        &self.instructions
    }

    pub fn literals(&self) -> &[Value] {
        &self.literals
    }

    /// Initialize the inline cache side table to match instruction count.
    /// Called once after instructions are finalized.
    pub fn init_cache(&mut self) {
        let len = self.instructions.len();
        self.cache = Vec::with_capacity(len);
        for _ in 0..len {
            self.cache.push(InlineCache::empty());
        }
    }
}

fn op_array_supports_cleanup_fast(op_array: &OpArray) -> bool {
    if !op_array.global_vars.is_empty()
        || !op_array.static_vars.is_empty()
        || !op_array.try_entries.is_empty()
        || op_array.is_generator
    {
        return false;
    }

    op_array.instructions.iter().all(|instr| {
        matches!(
            instr.opcode,
            OpCode::Add
                | OpCode::Sub
                | OpCode::Mul
                | OpCode::Div
                | OpCode::Mod
                | OpCode::Pow
                | OpCode::BitwiseAnd
                | OpCode::BitwiseOr
                | OpCode::BitwiseXor
                | OpCode::BitwiseNot
                | OpCode::ShiftLeft
                | OpCode::ShiftRight
                | OpCode::IsEqual
                | OpCode::IsNotEqual
                | OpCode::IsSmaller
                | OpCode::IsSmallerOrEqual
                | OpCode::IsIdentical
                | OpCode::IsNotIdentical
                | OpCode::BoolNot
                | OpCode::Spaceship
                | OpCode::Echo
                | OpCode::InitFcall
                | OpCode::InitMethodCall
                | OpCode::InitStaticCall
                | OpCode::InitDynamicCall
                | OpCode::SendVal
                | OpCode::SendRef
                | OpCode::SendVarEx
                | OpCode::SendNamed
                | OpCode::DoFcall
                | OpCode::Return
                | OpCode::Jmp
                | OpCode::JmpZ
                | OpCode::JmpNZ
                | OpCode::NullSafeCheck
                | OpCode::Instanceof
        )
    })
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
pub fn make_user_function_full(mut op_array: OpArray, num_args: u32, required_num_args: u32, is_variadic: bool, variadic_cv_index: u32, ref_args: u64) -> UserFunction {
    if op_array.cache.len() != op_array.instructions.len() {
        op_array.init_cache();
    }
    let call = if !is_variadic { CallStrategy::Fast } else { CallStrategy::Full };
    let cleanup = if op_array_supports_cleanup_fast(&op_array) { CleanupMode::SkipScan } else { CleanupMode::ScanAll };
    let ret = if op_array.global_vars.is_empty()
        && op_array.static_vars.is_empty()
        && op_array.try_entries.is_empty()
        && !op_array.is_generator
    { ReturnStrategy::Fast } else { ReturnStrategy::Full };
    let num_cvs = op_array.num_cvs;
    let num_temps = op_array.num_temps;
    let total_slots = crate::vm::frame::CALL_FRAME_SLOTS as u32 + num_cvs + num_temps;
    UserFunction {
        common: FunctionCommon {
            fn_type: FunctionType::User,
            sig: SignatureInfo {
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
            frame: FrameLayout { num_cvs, num_temps, total_slots },
            plan: CallPlan { call, ret, cleanup },
        },
        op_array,
    }
}

/// Extended full constructor with type hints and param names.
pub fn make_user_function_typed(
    mut op_array: OpArray,
    num_args: u32,
    required_num_args: u32,
    is_variadic: bool,
    variadic_cv_index: u32,
    ref_args: u64,
    param_type_hints: Vec<ParamTypeHint>,
    param_names: Vec<String>,
    return_type_hint: ParamTypeHint,
) -> UserFunction {
    if op_array.cache.len() != op_array.instructions.len() {
        op_array.init_cache();
    }
    let call = if !is_variadic && param_type_hints.iter().all(|h| matches!(h, ParamTypeHint::None)) {
        CallStrategy::Fast
    } else {
        CallStrategy::Full
    };
    let cleanup = if op_array_supports_cleanup_fast(&op_array) { CleanupMode::SkipScan } else { CleanupMode::ScanAll };
    let ret = if op_array.global_vars.is_empty()
        && op_array.static_vars.is_empty()
        && op_array.try_entries.is_empty()
        && !op_array.is_generator
        && matches!(return_type_hint, ParamTypeHint::None)
    { ReturnStrategy::Fast } else { ReturnStrategy::Full };
    let num_cvs = op_array.num_cvs;
    let num_temps = op_array.num_temps;
    let total_slots = crate::vm::frame::CALL_FRAME_SLOTS as u32 + num_cvs + num_temps;
    UserFunction {
        common: FunctionCommon {
            fn_type: FunctionType::User,
            sig: SignatureInfo {
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
            frame: FrameLayout { num_cvs, num_temps, total_slots },
            plan: CallPlan { call, ret, cleanup },
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
    let total_slots = crate::vm::frame::CALL_FRAME_SLOTS as u32 + num_args;
    InternalFunction {
        common: FunctionCommon {
            fn_type: FunctionType::Internal,
            sig: SignatureInfo {
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
            frame: FrameLayout { num_cvs: num_args, num_temps: 0, total_slots },
            plan: CallPlan { call: CallStrategy::Full, ret: ReturnStrategy::Full, cleanup: CleanupMode::ScanAll },
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
    let total_slots = crate::vm::frame::CALL_FRAME_SLOTS as u32 + num_args;
    InternalFunction {
        common: FunctionCommon {
            fn_type: FunctionType::Internal,
            sig: SignatureInfo {
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
            frame: FrameLayout { num_cvs: num_args, num_temps: 0, total_slots },
            plan: CallPlan { call: CallStrategy::Full, ret: ReturnStrategy::Full, cleanup: CleanupMode::ScanAll },
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
    let total_slots = crate::vm::frame::CALL_FRAME_SLOTS as u32 + num_args;
    InternalFunction {
        common: FunctionCommon {
            fn_type: FunctionType::Internal,
            sig: SignatureInfo {
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
            frame: FrameLayout { num_cvs: num_args, num_temps: 0, total_slots },
            plan: CallPlan { call: CallStrategy::Full, ret: ReturnStrategy::Full, cleanup: CleanupMode::ScanAll },
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
    let num_cvs = required_num_args + 1;
    let total_slots = crate::vm::frame::CALL_FRAME_SLOTS as u32 + num_cvs;
    InternalFunction {
        common: FunctionCommon {
            fn_type: FunctionType::Internal,
            sig: SignatureInfo {
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
            frame: FrameLayout { num_cvs, num_temps: 0, total_slots },
            plan: CallPlan { call: CallStrategy::Full, ret: ReturnStrategy::Full, cleanup: CleanupMode::ScanAll },
        },
        handler,
    }
}
