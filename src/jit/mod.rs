//! Native-code backend for proven hot regions.
//!
//! RPHP emits machine code itself, seals the resulting memory as executable,
//! and calls it through the platform ABI. Native code is always guarded by the
//! shared typed-region contract and retains an exact canonical fallback.

mod runtime;
mod straight;

pub use runtime::{
    DEFAULT_CODE_MAPPING_LIMIT_BYTES, MAX_CODE_MAPPING_LIMIT_BYTES, RuntimeTelemetry, enabled,
    telemetry,
};

#[cfg(any(
    all(target_arch = "aarch64", target_os = "macos"),
    all(target_arch = "x86_64", target_os = "linux")
))]
mod memory;

#[cfg(all(target_arch = "x86_64", target_os = "linux"))]
mod x86_64;

#[cfg(all(target_arch = "x86_64", target_os = "linux"))]
pub use x86_64::{
    CompiledQuickDoubleCallAccumulateLoop, CompiledScalarDoubleProgram, CompiledScalarLongProgram,
    CompiledX86AddMultiply, CompiledX86StraightLongLoop, NativeDoubleCallAccumulateState,
    QuickDoubleCallAccumulateJitCache, QuickDoubleCallAccumulateJitError,
    QuickDoubleCallAccumulateJitOutcome, SCALAR_DOUBLE_JIT_HOT_THRESHOLD,
    SCALAR_LONG_JIT_HOT_THRESHOLD, ScalarDoubleJitCache, ScalarDoubleJitDispatch,
    ScalarDoubleJitError, ScalarDoubleJitOutcome, ScalarLongJitCache, ScalarLongJitDispatch,
    ScalarLongJitError, ScalarLongJitOutcome, X86_64Assembler, X86_64FloatRegister, X86_64Register,
    X86QuickLongOpsJitCache as QuickLongAccumulateJitCache,
    X86QuickLongOpsJitCache as QuickLongOpsJitCache, X86StraightLongLoopError,
};

pub use straight::{
    NATIVE_QUICK_LONG_MAX_CALL_TARGETS, NATIVE_STRAIGHT_LONG_MAX_CONTEXT_ENTRIES,
    NATIVE_STRAIGHT_LONG_MAX_OPERATIONS, NativeStraightLongConditionOperand,
    NativeStraightLongLoopConfig, NativeStraightLongLoopOutcome, NativeStraightLongLoopResult,
    NativeStraightLongOperation,
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
    CompiledQuickDoubleCallAccumulateLoop, CompiledQuickLongAccumulateLoop,
    CompiledQuickLongConditionalAccumulateLoop, CompiledQuickLongStraightLoop,
    CompiledScalarDoubleProgram, CompiledScalarLongProgram, NativeConditionalLongLoopCondition,
    NativeConditionalLongLoopConfig, NativeConditionalLongLoopResult,
    NativeDoubleCallAccumulateState, NativeLongAccumulateState, QuickDoubleCallAccumulateJitCache,
    QuickDoubleCallAccumulateJitError, QuickDoubleCallAccumulateJitOutcome,
    QuickLongAccumulateJitCache, QuickLongAccumulateJitError, QuickLongAccumulateJitOutcome,
    QuickLongOpsJitCache, SCALAR_DOUBLE_JIT_HOT_THRESHOLD, SCALAR_LONG_JIT_HOT_THRESHOLD,
    ScalarDoubleJitCache, ScalarDoubleJitDispatch, ScalarDoubleJitError, ScalarDoubleJitOutcome,
    ScalarLongJitCache, ScalarLongJitDispatch, ScalarLongJitError, ScalarLongJitOutcome,
};
