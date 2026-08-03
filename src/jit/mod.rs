//! Experimental native-code backend.
//!
//! The first slice deliberately proves only the platform boundary: RPHP emits
//! ARM64 instructions itself, seals the resulting memory as executable, and
//! calls it through the platform ABI. It is feature-gated and is not connected
//! to PHP execution until typed guards and exact side exits can be preserved.

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
    Arm64Assembler, Arm64Register, CompiledAddMultiply,
    CompiledQuickLongAccumulateLoop, CompiledQuickLongConditionalAccumulateLoop,
    CompiledScalarLongProgram, NativeConditionalLongLoopConfig, NativeLongAccumulateState,
    QuickLongAccumulateJitCache, QuickLongAccumulateJitError, QuickLongAccumulateJitOutcome,
    QuickLongOpsJitCache, SCALAR_LONG_JIT_HOT_THRESHOLD, ScalarLongJitCache,
    ScalarLongJitDispatch, ScalarLongJitError, ScalarLongJitOutcome,
};
