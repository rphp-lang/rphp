#![cfg(all(
    feature = "jit-prototype",
    target_arch = "aarch64",
    target_os = "macos"
))]

mod common;

use rphp::compiler::compile::Compiler;
use rphp::compiler::make_user_function;
use rphp::jit::{
    Arm64Assembler, Arm64Register, CompiledAddMultiply, CompiledQuickLongAccumulateLoop,
    CompiledQuickLongConditionalAccumulateLoop, CompiledQuickLongStraightLoop,
    CompiledScalarLongProgram, NATIVE_STRAIGHT_LONG_MAX_CONTEXT_ENTRIES,
    NATIVE_STRAIGHT_LONG_MAX_OPERATIONS, NativeConditionalLongLoopCondition,
    NativeConditionalLongLoopConfig, NativeLongAccumulateState, NativeStraightLongConditionOperand,
    NativeStraightLongLoopConfig, NativeStraightLongLoopOutcome, NativeStraightLongOperation,
    QuickLongAccumulateJitError, QuickLongAccumulateJitOutcome, SCALAR_DOUBLE_JIT_HOT_THRESHOLD,
    SCALAR_LONG_JIT_HOT_THRESHOLD, ScalarLongJitDispatch, ScalarLongJitError, ScalarLongJitOutcome,
};
use rphp::lexer::Lexer;
use rphp::parser::Parser;
use rphp::vm::execute;
use rphp::vm::function::{
    FunctionCommon, ScalarLongConditionKind, ScalarLongConditionOperand, ScalarLongFunctionPlan,
    ScalarLongOp, ScalarLongOpKind, ScalarLongProgram, ScalarLongSelect, ScalarLongSource,
};
use rphp::vm::planner::BlockPlan;
use rphp::vm::quick::{QuickLongOp, QuickLongOperand};

fn scalar_plan(
    public_args: u8,
    operations: Vec<ScalarLongOp>,
    output: ScalarLongSource,
) -> ScalarLongFunctionPlan {
    ScalarLongFunctionPlan::new(
        public_args,
        ScalarLongProgram {
            operations: operations.into_boxed_slice(),
            outputs: [output],
            output_count: 1,
        },
        None,
    )
}

include!("jit_aarch64_prototype/double_runtime.rs");
include!("jit_aarch64_prototype/scalar_codegen.rs");
include!("jit_aarch64_prototype/guarded_runtime.rs");
include!("jit_aarch64_prototype/scalar_calls.rs");
include!("jit_aarch64_prototype/straight_codegen.rs");
include!("jit_aarch64_prototype/straight_runtime.rs");
