//! Experimental native-code backend.
//!
//! The first slice deliberately proves only the platform boundary: RPHP emits
//! ARM64 instructions itself, seals the resulting memory as executable, and
//! calls it through the platform ABI. It is feature-gated and is not connected
//! to PHP execution until typed guards and exact side exits can be preserved.

mod straight;

#[cfg(any(
    all(target_arch = "aarch64", target_os = "macos"),
    all(target_arch = "x86_64", target_os = "linux")
))]
mod memory;

#[cfg(all(target_arch = "x86_64", target_os = "linux"))]
mod x86_64;

#[cfg(all(target_arch = "x86_64", target_os = "linux"))]
pub use x86_64::{
    CompiledScalarDoubleProgram, CompiledScalarLongProgram, CompiledX86AddMultiply,
    CompiledX86StraightLongLoop, SCALAR_DOUBLE_JIT_HOT_THRESHOLD,
    SCALAR_LONG_JIT_HOT_THRESHOLD, ScalarDoubleJitCache, ScalarDoubleJitDispatch,
    ScalarDoubleJitError, ScalarDoubleJitOutcome, ScalarLongJitCache, ScalarLongJitDispatch,
    ScalarLongJitError, ScalarLongJitOutcome, X86_64Assembler, X86_64Register,
    X86_64FloatRegister,
    CompiledQuickDoubleCallAccumulateLoop, NativeDoubleCallAccumulateState,
    QuickDoubleCallAccumulateJitCache, QuickDoubleCallAccumulateJitError,
    QuickDoubleCallAccumulateJitOutcome,
    X86QuickLongOpsJitCache as QuickLongAccumulateJitCache,
    X86QuickLongOpsJitCache as QuickLongOpsJitCache, X86StraightLongLoopError,
};

pub use straight::{
    NATIVE_QUICK_LONG_MAX_CALL_TARGETS, NATIVE_STRAIGHT_LONG_MAX_CONTEXT_ENTRIES,
    NATIVE_STRAIGHT_LONG_MAX_OPERATIONS,
    NativeStraightLongConditionOperand, NativeStraightLongLoopConfig,
    NativeStraightLongLoopOutcome, NativeStraightLongLoopResult, NativeStraightLongOperation,
};

#[cfg(all(
    feature = "jit-prototype",
    target_arch = "aarch64",
    target_os = "macos"
))]
mod aarch64;

#[cfg(all(
    feature = "jit-prototype",
    target_arch = "aarch64",
    target_os = "macos"
))]
pub use aarch64::{
    Arm64Assembler, Arm64FloatRegister, Arm64Register, CompiledAddMultiply,
    CompiledQuickLongAccumulateLoop, CompiledQuickLongConditionalAccumulateLoop,
    CompiledQuickLongStraightLoop, CompiledScalarDoubleProgram, CompiledScalarLongProgram,
    NativeConditionalLongLoopCondition,
    NativeConditionalLongLoopConfig, NativeConditionalLongLoopResult,
    NativeLongAccumulateState, QuickLongAccumulateJitCache, QuickLongAccumulateJitError,
    QuickLongAccumulateJitOutcome,
    CompiledQuickDoubleCallAccumulateLoop, NativeDoubleCallAccumulateState,
    QuickDoubleCallAccumulateJitCache, QuickDoubleCallAccumulateJitError,
    QuickDoubleCallAccumulateJitOutcome,
    QuickLongOpsJitCache, SCALAR_DOUBLE_JIT_HOT_THRESHOLD, SCALAR_LONG_JIT_HOT_THRESHOLD,
    ScalarDoubleJitCache, ScalarDoubleJitDispatch, ScalarDoubleJitError,
    ScalarDoubleJitOutcome, ScalarLongJitCache, ScalarLongJitDispatch, ScalarLongJitError,
    ScalarLongJitOutcome,
};
