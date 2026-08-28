// VM runtime statistics — compile-time gated behind `vm-stats` feature.
// Without the feature, all functions compile to nothing (zero overhead).
// Usage: cargo build --features vm-stats && RPHP_VM_STATS=1 ./target/release/rphp script.php

/// Region shapes admitted by the shared quick-loop/JIT planner.
///
/// The planner is shared by the no-JIT typed executor and the native JIT, so
/// these counters describe optimization coverage rather than native code alone.
#[repr(u8)]
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum JitRegionKind {
    LongInduction = 0,
    DoubleCallAccumulate = 1,
    LongAccumulate = 2,
    ForeachLongAccumulate = 3,
    TypedOpsLoop = 4,
    StraightArrayRegion = 5,
    ScalarLongFunction = 6,
    ScalarDoubleFunction = 7,
    ForeachObjectPropertyAccumulate = 8,
}

impl JitRegionKind {
    #[cfg(feature = "vm-stats")]
    const COUNT: usize = 9;

    #[cfg(feature = "vm-stats")]
    #[inline(always)]
    const fn index(self) -> usize {
        self as usize
    }
}

/// Dominant operation family in a loop the quick-loop/JIT planner rejected.
///
/// This deliberately describes the first architectural gap, not an exact
/// parser error. One loop can contain several unsupported families; the
/// classifier uses a stable priority so corpus reports remain comparable.
#[repr(u8)]
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum JitMissReason {
    JsonPipeline = 0,
    CallbackOrIndirectCall = 1,
    ArrayShape = 2,
    StringShape = 3,
    ObjectShape = 4,
    DirectCallShape = 5,
    SemanticBoundary = 6,
    ComplexControlFlow = 7,
    UnsupportedScalarShape = 8,
}

impl JitMissReason {
    #[cfg(feature = "vm-stats")]
    const COUNT: usize = 9;

    #[cfg(feature = "vm-stats")]
    #[inline(always)]
    const fn index(self) -> usize {
        self as usize
    }

    /// Non-zero marker stored in otherwise-unused `Jmp::extended_value` by a
    /// vm-stats build. Zero remains "not a measured rejected backedge".
    #[inline(always)]
    pub const fn marker(self) -> u32 {
        self as u32 + 1
    }

    #[cfg(feature = "vm-stats")]
    #[inline(always)]
    const fn from_marker(marker: u32) -> Option<Self> {
        match marker {
            1 => Some(Self::JsonPipeline),
            2 => Some(Self::CallbackOrIndirectCall),
            3 => Some(Self::ArrayShape),
            4 => Some(Self::StringShape),
            5 => Some(Self::ObjectShape),
            6 => Some(Self::DirectCallShape),
            7 => Some(Self::SemanticBoundary),
            8 => Some(Self::ComplexControlFlow),
            9 => Some(Self::UnsupportedScalarShape),
            _ => None,
        }
    }
}

/// Exact admission stage that rejected a straight-line application region.
#[repr(u8)]
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum JitStraightMissReason {
    NoTypedSpan = 0,
    NoDenseKernel = 1,
}

impl JitStraightMissReason {
    #[cfg(feature = "vm-stats")]
    const COUNT: usize = 2;

    #[cfg(feature = "vm-stats")]
    #[inline(always)]
    const fn index(self) -> usize {
        self as usize
    }
}

#[cfg(feature = "vm-stats")]
mod inner {
    use super::{JitMissReason, JitRegionKind, JitStraightMissReason};
    use std::io::Write;
    use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};

    const VALUE_KIND_COUNT: usize = 12;
    const OPCODE_KIND_COUNT: usize = 256;

    static ENABLED: AtomicBool = AtomicBool::new(false);

    static PUSH_CALL_FRAME_CALLS: AtomicU64 = AtomicU64::new(0);
    static PUSH_CALL_FRAME_ZERO_SLOTS: AtomicU64 = AtomicU64::new(0);
    static PUSH_CALL_FRAME_ZERO_BYTES: AtomicU64 = AtomicU64::new(0);

    static CLEANUP_FRAME_CALLS: AtomicU64 = AtomicU64::new(0);
    static CLEANUP_FRAME_FAST_SKIPS: AtomicU64 = AtomicU64::new(0);
    static CLEANUP_FRAME_SCANNED_SLOTS: AtomicU64 = AtomicU64::new(0);

    static WRITE_VAL_CALLS: AtomicU64 = AtomicU64::new(0);
    static WRITE_FRAME_SLOT_CALLS: AtomicU64 = AtomicU64::new(0);
    static WRITE_FRAME_SLOT_HEAP_VALUES: AtomicU64 = AtomicU64::new(0);

    static DO_FCALL_FAST_PATHS: AtomicU64 = AtomicU64::new(0);
    static DO_FCALL_FULL_PATHS: AtomicU64 = AtomicU64::new(0);
    static RETURN_FAST_PATHS: AtomicU64 = AtomicU64::new(0);
    static RETURN_FULL_PATHS: AtomicU64 = AtomicU64::new(0);

    static QUICK_LOOP_ENTRIES: AtomicU64 = AtomicU64::new(0);
    static QUICK_LOOP_COMPLETIONS: AtomicU64 = AtomicU64::new(0);
    static QUICK_LOOP_DEOPTIMIZATIONS: AtomicU64 = AtomicU64::new(0);
    static QUICK_LOOP_GUARD_FAILURES: AtomicU64 = AtomicU64::new(0);
    static QUICK_LOOP_ITERATIONS: AtomicU64 = AtomicU64::new(0);
    static QUICK_PACKED_ARRAY_RESERVE_ATTEMPTS: AtomicU64 = AtomicU64::new(0);
    static QUICK_PACKED_ARRAY_RESERVE_SUCCESSES: AtomicU64 = AtomicU64::new(0);
    static QUICK_PACKED_ARRAY_RESERVE_ENTRIES: AtomicU64 = AtomicU64::new(0);

    static JIT_LOOP_CANDIDATES: AtomicU64 = AtomicU64::new(0);
    static JIT_LOOP_ADMISSIONS: [AtomicU64; JitRegionKind::COUNT] =
        [const { AtomicU64::new(0) }; JitRegionKind::COUNT];
    static JIT_LOOP_REJECTIONS: [AtomicU64; JitMissReason::COUNT] =
        [const { AtomicU64::new(0) }; JitMissReason::COUNT];
    static JIT_REJECTED_BACKEDGE_HITS: [AtomicU64; JitMissReason::COUNT] =
        [const { AtomicU64::new(0) }; JitMissReason::COUNT];
    static JIT_REGION_EXECUTIONS: [AtomicU64; JitRegionKind::COUNT] =
        [const { AtomicU64::new(0) }; JitRegionKind::COUNT];
    static JIT_NATIVE_EXECUTIONS: [AtomicU64; JitRegionKind::COUNT] =
        [const { AtomicU64::new(0) }; JitRegionKind::COUNT];
    static JIT_NATIVE_SIDE_EXITS: [AtomicU64; JitRegionKind::COUNT] =
        [const { AtomicU64::new(0) }; JitRegionKind::COUNT];
    static JIT_STRAIGHT_CANDIDATES: AtomicU64 = AtomicU64::new(0);
    static JIT_STRAIGHT_ADMISSIONS: AtomicU64 = AtomicU64::new(0);
    static JIT_STRAIGHT_REJECTIONS: [AtomicU64; JitStraightMissReason::COUNT] =
        [const { AtomicU64::new(0) }; JitStraightMissReason::COUNT];

    static FIND_FUNCTION_CALLS: AtomicU64 = AtomicU64::new(0);
    static FIND_FUNCTION_EXACT_HITS: AtomicU64 = AtomicU64::new(0);
    static FIND_FUNCTION_LOWER_HITS: AtomicU64 = AtomicU64::new(0);
    static FIND_FUNCTION_MISSES: AtomicU64 = AtomicU64::new(0);

    static VALUE_CLONES: [AtomicU64; VALUE_KIND_COUNT] =
        [const { AtomicU64::new(0) }; VALUE_KIND_COUNT];
    static VALUE_DROPS: [AtomicU64; VALUE_KIND_COUNT] =
        [const { AtomicU64::new(0) }; VALUE_KIND_COUNT];
    static ARRAY_OWNER_ALLOCATIONS: AtomicU64 = AtomicU64::new(0);
    static CLOSURE_PAYLOAD_ALLOCATIONS: AtomicU64 = AtomicU64::new(0);
    static CLOSURE_CAPTURE_STORAGE_ALLOCATIONS: AtomicU64 = AtomicU64::new(0);
    static DECLARED_OBJECT_OWNER_ALLOCATIONS: AtomicU64 = AtomicU64::new(0);
    static DECLARED_PROPERTY_STORAGE_ALLOCATIONS: AtomicU64 = AtomicU64::new(0);
    static DECLARED_PROPERTY_STORAGE_REUSES: AtomicU64 = AtomicU64::new(0);
    static DECLARED_PROPERTY_STORAGE_RETURNS: AtomicU64 = AtomicU64::new(0);
    static NEWOBJ_LITERAL_CACHE_HITS: AtomicU64 = AtomicU64::new(0);
    static NEWOBJ_LITERAL_CACHE_MISSES: AtomicU64 = AtomicU64::new(0);
    static NEWOBJ_CLASS_NAME_MATERIALIZATIONS: AtomicU64 = AtomicU64::new(0);
    static NEWOBJ_CLASS_HASH_LOOKUPS: AtomicU64 = AtomicU64::new(0);
    static RESOLVED_VIRTUAL_AGGREGATE_RESOLVE_ATTEMPTS: AtomicU64 = AtomicU64::new(0);
    static RESOLVED_VIRTUAL_AGGREGATE_RESOLVE_SUCCESSES: AtomicU64 = AtomicU64::new(0);
    static RESOLVED_VIRTUAL_AGGREGATE_CACHE_HITS: AtomicU64 = AtomicU64::new(0);
    static RESOLVED_VIRTUAL_AGGREGATE_CACHE_INVALIDATIONS: AtomicU64 = AtomicU64::new(0);
    static RESOLVED_VIRTUAL_AGGREGATE_GUARD_FALLBACKS: AtomicU64 = AtomicU64::new(0);
    static OPCODE_COUNTS: [AtomicU64; OPCODE_KIND_COUNT] =
        [const { AtomicU64::new(0) }; OPCODE_KIND_COUNT];

    #[inline]
    pub fn configure_from_env() {
        let enabled = std::env::var_os("RPHP_VM_STATS").is_some();
        ENABLED.store(enabled, Ordering::Relaxed);
    }

    #[inline]
    pub fn enabled() -> bool {
        ENABLED.load(Ordering::Relaxed)
    }

    pub fn reset() {
        PUSH_CALL_FRAME_CALLS.store(0, Ordering::Relaxed);
        PUSH_CALL_FRAME_ZERO_SLOTS.store(0, Ordering::Relaxed);
        PUSH_CALL_FRAME_ZERO_BYTES.store(0, Ordering::Relaxed);
        CLEANUP_FRAME_CALLS.store(0, Ordering::Relaxed);
        CLEANUP_FRAME_FAST_SKIPS.store(0, Ordering::Relaxed);
        CLEANUP_FRAME_SCANNED_SLOTS.store(0, Ordering::Relaxed);
        WRITE_VAL_CALLS.store(0, Ordering::Relaxed);
        WRITE_FRAME_SLOT_CALLS.store(0, Ordering::Relaxed);
        WRITE_FRAME_SLOT_HEAP_VALUES.store(0, Ordering::Relaxed);
        DO_FCALL_FAST_PATHS.store(0, Ordering::Relaxed);
        DO_FCALL_FULL_PATHS.store(0, Ordering::Relaxed);
        RETURN_FAST_PATHS.store(0, Ordering::Relaxed);
        RETURN_FULL_PATHS.store(0, Ordering::Relaxed);
        QUICK_LOOP_ENTRIES.store(0, Ordering::Relaxed);
        QUICK_LOOP_COMPLETIONS.store(0, Ordering::Relaxed);
        QUICK_LOOP_DEOPTIMIZATIONS.store(0, Ordering::Relaxed);
        QUICK_LOOP_GUARD_FAILURES.store(0, Ordering::Relaxed);
        QUICK_LOOP_ITERATIONS.store(0, Ordering::Relaxed);
        QUICK_PACKED_ARRAY_RESERVE_ATTEMPTS.store(0, Ordering::Relaxed);
        QUICK_PACKED_ARRAY_RESERVE_SUCCESSES.store(0, Ordering::Relaxed);
        QUICK_PACKED_ARRAY_RESERVE_ENTRIES.store(0, Ordering::Relaxed);
        JIT_LOOP_CANDIDATES.store(0, Ordering::Relaxed);
        JIT_STRAIGHT_CANDIDATES.store(0, Ordering::Relaxed);
        JIT_STRAIGHT_ADMISSIONS.store(0, Ordering::Relaxed);
        for counter in &JIT_STRAIGHT_REJECTIONS {
            counter.store(0, Ordering::Relaxed);
        }
        for counter in &JIT_LOOP_ADMISSIONS {
            counter.store(0, Ordering::Relaxed);
        }
        for counter in &JIT_LOOP_REJECTIONS {
            counter.store(0, Ordering::Relaxed);
        }
        for counter in &JIT_REJECTED_BACKEDGE_HITS {
            counter.store(0, Ordering::Relaxed);
        }
        for counter in &JIT_REGION_EXECUTIONS {
            counter.store(0, Ordering::Relaxed);
        }
        for counter in &JIT_NATIVE_EXECUTIONS {
            counter.store(0, Ordering::Relaxed);
        }
        for counter in &JIT_NATIVE_SIDE_EXITS {
            counter.store(0, Ordering::Relaxed);
        }
        FIND_FUNCTION_CALLS.store(0, Ordering::Relaxed);
        FIND_FUNCTION_EXACT_HITS.store(0, Ordering::Relaxed);
        FIND_FUNCTION_LOWER_HITS.store(0, Ordering::Relaxed);
        FIND_FUNCTION_MISSES.store(0, Ordering::Relaxed);
        for counter in &VALUE_CLONES {
            counter.store(0, Ordering::Relaxed);
        }
        for counter in &VALUE_DROPS {
            counter.store(0, Ordering::Relaxed);
        }
        ARRAY_OWNER_ALLOCATIONS.store(0, Ordering::Relaxed);
        CLOSURE_PAYLOAD_ALLOCATIONS.store(0, Ordering::Relaxed);
        CLOSURE_CAPTURE_STORAGE_ALLOCATIONS.store(0, Ordering::Relaxed);
        DECLARED_OBJECT_OWNER_ALLOCATIONS.store(0, Ordering::Relaxed);
        DECLARED_PROPERTY_STORAGE_ALLOCATIONS.store(0, Ordering::Relaxed);
        DECLARED_PROPERTY_STORAGE_REUSES.store(0, Ordering::Relaxed);
        DECLARED_PROPERTY_STORAGE_RETURNS.store(0, Ordering::Relaxed);
        NEWOBJ_LITERAL_CACHE_HITS.store(0, Ordering::Relaxed);
        NEWOBJ_LITERAL_CACHE_MISSES.store(0, Ordering::Relaxed);
        NEWOBJ_CLASS_NAME_MATERIALIZATIONS.store(0, Ordering::Relaxed);
        NEWOBJ_CLASS_HASH_LOOKUPS.store(0, Ordering::Relaxed);
        RESOLVED_VIRTUAL_AGGREGATE_RESOLVE_ATTEMPTS.store(0, Ordering::Relaxed);
        RESOLVED_VIRTUAL_AGGREGATE_RESOLVE_SUCCESSES.store(0, Ordering::Relaxed);
        RESOLVED_VIRTUAL_AGGREGATE_CACHE_HITS.store(0, Ordering::Relaxed);
        RESOLVED_VIRTUAL_AGGREGATE_CACHE_INVALIDATIONS.store(0, Ordering::Relaxed);
        RESOLVED_VIRTUAL_AGGREGATE_GUARD_FALLBACKS.store(0, Ordering::Relaxed);
        for counter in &OPCODE_COUNTS {
            counter.store(0, Ordering::Relaxed);
        }
    }

    #[inline]
    pub fn inc_push_call_frame(slot_count: usize, zero_bytes: usize) {
        if !enabled() {
            return;
        }
        PUSH_CALL_FRAME_CALLS.fetch_add(1, Ordering::Relaxed);
        PUSH_CALL_FRAME_ZERO_SLOTS.fetch_add(slot_count as u64, Ordering::Relaxed);
        PUSH_CALL_FRAME_ZERO_BYTES.fetch_add(zero_bytes as u64, Ordering::Relaxed);
    }

    #[inline]
    pub fn inc_cleanup_frame(slot_count: usize, skipped: bool) {
        if !enabled() {
            return;
        }
        CLEANUP_FRAME_CALLS.fetch_add(1, Ordering::Relaxed);
        if skipped {
            CLEANUP_FRAME_FAST_SKIPS.fetch_add(1, Ordering::Relaxed);
        } else {
            CLEANUP_FRAME_SCANNED_SLOTS.fetch_add(slot_count as u64, Ordering::Relaxed);
        }
    }

    #[inline]
    pub fn inc_write_val() {
        if enabled() {
            WRITE_VAL_CALLS.fetch_add(1, Ordering::Relaxed);
        }
    }

    #[inline]
    pub fn inc_write_frame_slot(heap_value: bool) {
        if !enabled() {
            return;
        }
        WRITE_FRAME_SLOT_CALLS.fetch_add(1, Ordering::Relaxed);
        if heap_value {
            WRITE_FRAME_SLOT_HEAP_VALUES.fetch_add(1, Ordering::Relaxed);
        }
    }

    #[inline]
    pub fn inc_do_fcall_fast() {
        if enabled() {
            DO_FCALL_FAST_PATHS.fetch_add(1, Ordering::Relaxed);
        }
    }

    #[inline]
    pub fn inc_do_fcall_fast_by(count: u64) {
        if enabled() {
            DO_FCALL_FAST_PATHS.fetch_add(count, Ordering::Relaxed);
        }
    }

    #[inline]
    pub fn inc_do_fcall_full() {
        if enabled() {
            DO_FCALL_FULL_PATHS.fetch_add(1, Ordering::Relaxed);
        }
    }

    #[inline]
    pub fn inc_return_fast() {
        if enabled() {
            RETURN_FAST_PATHS.fetch_add(1, Ordering::Relaxed);
        }
    }

    #[inline]
    pub fn inc_return_fast_by(count: u64) {
        if enabled() {
            RETURN_FAST_PATHS.fetch_add(count, Ordering::Relaxed);
        }
    }

    #[inline]
    pub fn inc_return_full() {
        if enabled() {
            RETURN_FULL_PATHS.fetch_add(1, Ordering::Relaxed);
        }
    }

    #[inline]
    pub fn inc_quick_loop_completed(iterations: u64) {
        if !enabled() {
            return;
        }
        QUICK_LOOP_ENTRIES.fetch_add(1, Ordering::Relaxed);
        QUICK_LOOP_COMPLETIONS.fetch_add(1, Ordering::Relaxed);
        QUICK_LOOP_ITERATIONS.fetch_add(iterations, Ordering::Relaxed);
    }

    #[inline]
    pub fn inc_quick_loop_deoptimized(iterations: u64) {
        if !enabled() {
            return;
        }
        QUICK_LOOP_ENTRIES.fetch_add(1, Ordering::Relaxed);
        QUICK_LOOP_DEOPTIMIZATIONS.fetch_add(1, Ordering::Relaxed);
        QUICK_LOOP_ITERATIONS.fetch_add(iterations, Ordering::Relaxed);
    }

    #[inline]
    pub fn inc_quick_loop_guard_failed() {
        if !enabled() {
            return;
        }
        QUICK_LOOP_GUARD_FAILURES.fetch_add(1, Ordering::Relaxed);
    }

    #[inline]
    pub fn record_quick_packed_array_reserve(entries: usize, succeeded: bool) {
        if !enabled() {
            return;
        }
        QUICK_PACKED_ARRAY_RESERVE_ATTEMPTS.fetch_add(1, Ordering::Relaxed);
        QUICK_PACKED_ARRAY_RESERVE_ENTRIES.fetch_add(entries as u64, Ordering::Relaxed);
        if succeeded {
            QUICK_PACKED_ARRAY_RESERVE_SUCCESSES.fetch_add(1, Ordering::Relaxed);
        }
    }

    #[inline]
    pub fn inc_jit_loop_candidate() {
        if enabled() {
            JIT_LOOP_CANDIDATES.fetch_add(1, Ordering::Relaxed);
        }
    }

    #[inline]
    pub fn inc_jit_loop_admitted(kind: JitRegionKind) {
        if enabled() {
            JIT_LOOP_ADMISSIONS[kind.index()].fetch_add(1, Ordering::Relaxed);
        }
    }

    #[inline]
    pub fn inc_jit_loop_rejected(reason: JitMissReason) {
        if enabled() {
            JIT_LOOP_REJECTIONS[reason.index()].fetch_add(1, Ordering::Relaxed);
        }
    }

    #[inline]
    pub fn inc_jit_rejected_backedge_hit(marker: u32) {
        if !enabled() {
            return;
        }
        if let Some(reason) = JitMissReason::from_marker(marker) {
            JIT_REJECTED_BACKEDGE_HITS[reason.index()].fetch_add(1, Ordering::Relaxed);
        }
    }

    #[inline]
    pub fn inc_jit_region_execution(kind: JitRegionKind) {
        if enabled() {
            JIT_REGION_EXECUTIONS[kind.index()].fetch_add(1, Ordering::Relaxed);
        }
    }

    #[inline]
    pub fn inc_jit_native_execution(kind: JitRegionKind) {
        if enabled() {
            JIT_NATIVE_EXECUTIONS[kind.index()].fetch_add(1, Ordering::Relaxed);
        }
    }

    #[inline]
    pub fn inc_jit_native_side_exit(kind: JitRegionKind) {
        if enabled() {
            JIT_NATIVE_SIDE_EXITS[kind.index()].fetch_add(1, Ordering::Relaxed);
        }
    }

    #[inline]
    pub fn inc_jit_straight_candidate() {
        if enabled() {
            JIT_STRAIGHT_CANDIDATES.fetch_add(1, Ordering::Relaxed);
        }
    }

    #[inline]
    pub fn inc_jit_straight_admitted() {
        if enabled() {
            JIT_STRAIGHT_ADMISSIONS.fetch_add(1, Ordering::Relaxed);
        }
    }

    #[inline]
    pub fn inc_jit_straight_rejected(reason: JitStraightMissReason) {
        if enabled() {
            JIT_STRAIGHT_REJECTIONS[reason.index()].fetch_add(1, Ordering::Relaxed);
        }
    }

    #[inline]
    pub fn inc_find_function_exact_hit() {
        if !enabled() {
            return;
        }
        FIND_FUNCTION_CALLS.fetch_add(1, Ordering::Relaxed);
        FIND_FUNCTION_EXACT_HITS.fetch_add(1, Ordering::Relaxed);
    }

    #[inline]
    pub fn inc_find_function_lower_hit() {
        if !enabled() {
            return;
        }
        FIND_FUNCTION_CALLS.fetch_add(1, Ordering::Relaxed);
        FIND_FUNCTION_LOWER_HITS.fetch_add(1, Ordering::Relaxed);
    }

    #[inline]
    pub fn inc_find_function_miss() {
        if !enabled() {
            return;
        }
        FIND_FUNCTION_CALLS.fetch_add(1, Ordering::Relaxed);
        FIND_FUNCTION_MISSES.fetch_add(1, Ordering::Relaxed);
    }

    #[inline]
    pub fn inc_value_clone(kind: usize) {
        if enabled() && kind < VALUE_KIND_COUNT {
            VALUE_CLONES[kind].fetch_add(1, Ordering::Relaxed);
        }
    }

    #[inline]
    pub fn inc_value_drop(kind: usize) {
        if enabled() && kind < VALUE_KIND_COUNT {
            VALUE_DROPS[kind].fetch_add(1, Ordering::Relaxed);
        }
    }

    #[inline]
    pub fn inc_array_owner_allocation() {
        if enabled() {
            ARRAY_OWNER_ALLOCATIONS.fetch_add(1, Ordering::Relaxed);
        }
    }

    #[inline]
    pub fn inc_closure_payload_allocation() {
        if enabled() {
            CLOSURE_PAYLOAD_ALLOCATIONS.fetch_add(1, Ordering::Relaxed);
        }
    }

    #[inline]
    pub fn inc_closure_capture_storage_allocation() {
        if enabled() {
            CLOSURE_CAPTURE_STORAGE_ALLOCATIONS.fetch_add(1, Ordering::Relaxed);
        }
    }

    #[inline]
    pub fn inc_declared_object_owner_allocation() {
        if enabled() {
            DECLARED_OBJECT_OWNER_ALLOCATIONS.fetch_add(1, Ordering::Relaxed);
        }
    }

    #[inline]
    pub fn inc_declared_property_storage_allocation() {
        if enabled() {
            DECLARED_PROPERTY_STORAGE_ALLOCATIONS.fetch_add(1, Ordering::Relaxed);
        }
    }

    #[inline]
    pub fn inc_declared_property_storage_reuse() {
        if enabled() {
            DECLARED_PROPERTY_STORAGE_REUSES.fetch_add(1, Ordering::Relaxed);
        }
    }

    #[inline]
    pub fn inc_declared_property_storage_return() {
        if enabled() {
            DECLARED_PROPERTY_STORAGE_RETURNS.fetch_add(1, Ordering::Relaxed);
        }
    }

    #[inline]
    pub fn inc_newobj_literal_cache_hit() {
        if enabled() {
            NEWOBJ_LITERAL_CACHE_HITS.fetch_add(1, Ordering::Relaxed);
        }
    }

    #[inline]
    pub fn inc_newobj_literal_cache_miss() {
        if enabled() {
            NEWOBJ_LITERAL_CACHE_MISSES.fetch_add(1, Ordering::Relaxed);
        }
    }

    #[inline]
    pub fn inc_newobj_class_name_materialization() {
        if enabled() {
            NEWOBJ_CLASS_NAME_MATERIALIZATIONS.fetch_add(1, Ordering::Relaxed);
        }
    }

    #[inline]
    pub fn inc_newobj_class_hash_lookup() {
        if enabled() {
            NEWOBJ_CLASS_HASH_LOOKUPS.fetch_add(1, Ordering::Relaxed);
        }
    }

    #[inline]
    pub fn inc_resolved_virtual_aggregate_resolve_attempt() {
        if enabled() {
            RESOLVED_VIRTUAL_AGGREGATE_RESOLVE_ATTEMPTS.fetch_add(1, Ordering::Relaxed);
        }
    }

    #[inline]
    pub fn inc_resolved_virtual_aggregate_resolve_success() {
        if enabled() {
            RESOLVED_VIRTUAL_AGGREGATE_RESOLVE_SUCCESSES.fetch_add(1, Ordering::Relaxed);
        }
    }

    #[inline]
    pub fn inc_resolved_virtual_aggregate_cache_hit() {
        if enabled() {
            RESOLVED_VIRTUAL_AGGREGATE_CACHE_HITS.fetch_add(1, Ordering::Relaxed);
        }
    }

    #[inline]
    pub fn inc_resolved_virtual_aggregate_cache_invalidation() {
        if enabled() {
            RESOLVED_VIRTUAL_AGGREGATE_CACHE_INVALIDATIONS.fetch_add(1, Ordering::Relaxed);
        }
    }

    #[inline]
    pub fn inc_resolved_virtual_aggregate_guard_fallback() {
        if enabled() {
            RESOLVED_VIRTUAL_AGGREGATE_GUARD_FALLBACKS.fetch_add(1, Ordering::Relaxed);
        }
    }

    #[inline]
    pub fn inc_opcode(opcode: usize) {
        if enabled() && opcode < OPCODE_KIND_COUNT {
            OPCODE_COUNTS[opcode].fetch_add(1, Ordering::Relaxed);
        }
    }

    fn value_kind_name(kind: usize) -> &'static str {
        match kind {
            0 => "undef",
            1 => "null",
            2 => "false",
            3 => "true",
            4 => "long",
            5 => "double",
            6 => "string",
            7 => "array",
            8 => "object",
            9 => "resource",
            10 => "reference",
            11 => "closure",
            _ => "unknown",
        }
    }

    fn opcode_name(op: usize) -> Option<&'static str> {
        match op {
            1 => Some("Add"),
            2 => Some("Sub"),
            3 => Some("Mul"),
            4 => Some("Div"),
            5 => Some("Mod"),
            8 => Some("Concat"),
            9 => Some("AssignConcat"),
            10 => Some("AssignCv"),
            11 => Some("FetchCvR"),
            13 => Some("BoolNot"),
            15 => Some("IsEqual"),
            16 => Some("IsNotEqual"),
            17 => Some("IsSmaller"),
            18 => Some("IsSmallerOrEqual"),
            19 => Some("IsIdentical"),
            20 => Some("IsNotIdentical"),
            21 => Some("Isset"),
            22 => Some("Cast"),
            34 => Some("PreInc"),
            35 => Some("PreDec"),
            36 => Some("PostInc"),
            37 => Some("PostDec"),
            40 => Some("Echo"),
            0 => Some("JmpFinally"),
            42 => Some("Jmp"),
            43 => Some("JmpZ"),
            44 => Some("JmpNZ"),
            60 => Some("DoFcall"),
            61 => Some("InitFcall"),
            62 => Some("Return"),
            63 => Some("SendVal"),
            64 => Some("SendRef"),
            65 => Some("SendVarEx"),
            66 => Some("SendNamed"),
            67 => Some("CallUserFuncArray"),
            68 => Some("InitUserCall"),
            69 => Some("SendUser"),
            226 => Some("SendUserChecked"),
            70 => Some("InitArray"),
            71 => Some("AddArrayElement"),
            72 => Some("FetchDimR"),
            73 => Some("AssignDim"),
            74 => Some("ArrayPushOp"),
            75 => Some("UnsetDim"),
            76 => Some("AddArrayUnpack"),
            77 => Some("AddCallArgument"),
            78 => Some("AddCallUnpack"),
            80 => Some("ForeachInit"),
            81 => Some("ForeachNext"),
            82 => Some("ForeachNextRef"),
            83 => Some("ForeachWriteback"),
            84 => Some("BindArrayAppendRef"),
            85 => Some("ForeachNextPlain"),
            90 => Some("Throw"),
            100 => Some("NewObj"),
            101 => Some("FetchObjR"),
            102 => Some("AssignObjProp"),
            103 => Some("InitMethodCall"),
            104 => Some("FetchStaticProp"),
            105 => Some("InitStaticCall"),
            106 => Some("InitDynamicCall"),
            107 => Some("Instanceof"),
            108 => Some("FetchConst"),
            109 => Some("BindDefaultParam"),
            110 => Some("Yield"),
            111 => Some("YieldFrom"),
            112 => Some("GeneratorReturn"),
            113 => Some("Spaceship"),
            114 => Some("Pow"),
            115 => Some("BitwiseAnd"),
            116 => Some("BitwiseOr"),
            117 => Some("BitwiseXor"),
            118 => Some("ShiftLeft"),
            119 => Some("ShiftRight"),
            120 => Some("BitwiseNot"),
            121 => Some("BindGlobal"),
            122 => Some("CheckStatic"),
            123 => Some("BindStatic"),
            124 => Some("AssignObjDim"),
            125 => Some("Include"),
            126 => Some("NullSafeCheck"),
            127 => Some("CloneObj"),
            128 => Some("CreateClosure"),
            129 => Some("ClosureUseVar"),
            130 => Some("DirectInternalCall1"),
            131 => Some("Strlen"),
            132 => Some("DirectInternalCall2"),
            133 => Some("CheckGenericArgs"),
            134 => Some("CheckReifiedArgs"),
            135 => Some("CheckReifiedReturn"),
            136 => Some("CheckGenericDefault"),
            137 => Some("InitLateStaticCall"),
            138 => Some("CheckLateStaticGenericArgs"),
            139 => Some("FetchLateStaticProp"),
            140 => Some("AssignStaticProp"),
            141 => Some("AssignLateStaticProp"),
            142 => Some("FetchClassConst"),
            143 => Some("FetchLateClassConst"),
            144 => Some("FetchDynamicClassConst"),
            145 => Some("FetchLateDynamicClassConst"),
            146 => Some("IssetObj"),
            147 => Some("UnsetObj"),
            148 => Some("CreateFirstClassCallable"),
            149 => Some("ReleaseTemps"),
            150 => Some("BindObjPropRef"),
            151 => Some("BindArrayDimRef"),
            152 => Some("FetchGlobal"),
            153 => Some("AssignGlobal"),
            154 => Some("UnsetGlobal"),
            155 => Some("BindGlobalRef"),
            156 => Some("AssignGlobalRef"),
            157 => Some("FetchGlobals"),
            158 => Some("FetchDynamicVar"),
            159 => Some("AssignDynamicVar"),
            160 => Some("UnsetDynamicVar"),
            161 => Some("BindDynamicVarRef"),
            162 => Some("AssignDynamicVarRef"),
            163 => Some("BindDynamicGlobal"),
            164 => Some("UnsetStaticProp"),
            165 => Some("BindCvRef"),
            166 => Some("Eval"),
            167 => Some("ValidateCloneWith"),
            168 => Some("AssertCheck"),
            169 => Some("EndCloneWith"),
            170 => Some("ReportDeprecatedTraitUses"),
            171 => Some("DeclareClass"),
            172 => Some("DeclarationCompileFatal"),
            200 => Some("Add_TmpTmp"),
            201 => Some("Sub_CvConst"),
            202 => Some("IsSmaller_CvConst"),
            203 => Some("IsSmallerOrEqual_CvConst"),
            204 => Some("Add_CvTmp"),
            205 => Some("Sub_TmpTmp"),
            206 => Some("JmpZ_Le_CvConst"),
            207 => Some("JmpNZ_Le_CvConst"),
            208 => Some("JmpZ_Lt_CvConst"),
            209 => Some("JmpNZ_Lt_CvConst"),
            210 => Some("IsEqual_CvConst"),
            211 => Some("JmpZ_Eq_CvConst"),
            212 => Some("JmpNZ_Eq_CvConst"),
            213 => Some("QuickLongLoopJmp"),
            214 => Some("Strlen_Cv"),
            215 => Some("Add_LongLong"),
            216 => Some("Sub_LongLong"),
            217 => Some("Mul_LongLong"),
            218 => Some("Mod_LongLong"),
            219 => Some("Concat_StringString"),
            220 => Some("Strlen_String"),
            221 => Some("Echo_String"),
            222 => Some("Echo_Long"),
            223 => Some("BitwiseXor_LongLong"),
            224 => Some("BitwiseAnd_LongLong"),
            225 => Some("BitwiseOr_LongLong"),
            _ => None,
        }
    }

    fn jit_region_name(kind: usize) -> &'static str {
        match kind {
            0 => "long_induction",
            1 => "double_call_accumulate",
            2 => "long_accumulate",
            3 => "foreach_long_accumulate",
            4 => "typed_ops_loop",
            5 => "straight_array_region",
            6 => "scalar_long_function",
            7 => "scalar_double_function",
            8 => "foreach_object_property_accumulate",
            _ => "unknown",
        }
    }

    fn jit_miss_name(reason: usize) -> &'static str {
        match reason {
            0 => "json_pipeline",
            1 => "callback_or_indirect_call",
            2 => "array_shape",
            3 => "string_shape",
            4 => "object_shape",
            5 => "direct_call_shape",
            6 => "semantic_boundary",
            7 => "complex_control_flow",
            8 => "unsupported_scalar_shape",
            _ => "unknown",
        }
    }

    fn jit_straight_miss_name(reason: usize) -> &'static str {
        match reason {
            0 => "no_typed_span",
            1 => "no_dense_kernel",
            _ => "unknown",
        }
    }

    pub fn dump_to_stderr() {
        if !enabled() {
            return;
        }

        let mut err = std::io::stderr().lock();
        let _ = writeln!(err, "\n=== RPHP VM Stats ===");
        let _ = writeln!(
            err,
            "push_call_frame_calls={}",
            PUSH_CALL_FRAME_CALLS.load(Ordering::Relaxed)
        );
        let _ = writeln!(
            err,
            "push_call_frame_zero_slots={}",
            PUSH_CALL_FRAME_ZERO_SLOTS.load(Ordering::Relaxed)
        );
        let _ = writeln!(
            err,
            "push_call_frame_zero_bytes={}",
            PUSH_CALL_FRAME_ZERO_BYTES.load(Ordering::Relaxed)
        );
        let _ = writeln!(
            err,
            "cleanup_frame_calls={}",
            CLEANUP_FRAME_CALLS.load(Ordering::Relaxed)
        );
        let _ = writeln!(
            err,
            "cleanup_frame_fast_skips={}",
            CLEANUP_FRAME_FAST_SKIPS.load(Ordering::Relaxed)
        );
        let _ = writeln!(
            err,
            "cleanup_frame_scanned_slots={}",
            CLEANUP_FRAME_SCANNED_SLOTS.load(Ordering::Relaxed)
        );
        let _ = writeln!(
            err,
            "write_val_calls={}",
            WRITE_VAL_CALLS.load(Ordering::Relaxed)
        );
        let _ = writeln!(
            err,
            "write_frame_slot_calls={}",
            WRITE_FRAME_SLOT_CALLS.load(Ordering::Relaxed)
        );
        let _ = writeln!(
            err,
            "write_frame_slot_heap_values={}",
            WRITE_FRAME_SLOT_HEAP_VALUES.load(Ordering::Relaxed)
        );
        let _ = writeln!(
            err,
            "do_fcall_fast_paths={}",
            DO_FCALL_FAST_PATHS.load(Ordering::Relaxed)
        );
        let _ = writeln!(
            err,
            "do_fcall_full_paths={}",
            DO_FCALL_FULL_PATHS.load(Ordering::Relaxed)
        );
        let _ = writeln!(
            err,
            "return_fast_paths={}",
            RETURN_FAST_PATHS.load(Ordering::Relaxed)
        );
        let _ = writeln!(
            err,
            "return_full_paths={}",
            RETURN_FULL_PATHS.load(Ordering::Relaxed)
        );
        let _ = writeln!(
            err,
            "quick_loop_entries={}",
            QUICK_LOOP_ENTRIES.load(Ordering::Relaxed)
        );
        let _ = writeln!(
            err,
            "quick_loop_completions={}",
            QUICK_LOOP_COMPLETIONS.load(Ordering::Relaxed)
        );
        let _ = writeln!(
            err,
            "quick_loop_deoptimizations={}",
            QUICK_LOOP_DEOPTIMIZATIONS.load(Ordering::Relaxed)
        );
        let _ = writeln!(
            err,
            "quick_loop_guard_failures={}",
            QUICK_LOOP_GUARD_FAILURES.load(Ordering::Relaxed)
        );
        let _ = writeln!(
            err,
            "quick_loop_iterations={}",
            QUICK_LOOP_ITERATIONS.load(Ordering::Relaxed)
        );
        let _ = writeln!(
            err,
            "quick_packed_array_reserve_attempts={}",
            QUICK_PACKED_ARRAY_RESERVE_ATTEMPTS.load(Ordering::Relaxed)
        );
        let _ = writeln!(
            err,
            "quick_packed_array_reserve_successes={}",
            QUICK_PACKED_ARRAY_RESERVE_SUCCESSES.load(Ordering::Relaxed)
        );
        let _ = writeln!(
            err,
            "quick_packed_array_reserve_entries={}",
            QUICK_PACKED_ARRAY_RESERVE_ENTRIES.load(Ordering::Relaxed)
        );
        #[cfg(all(
            feature = "jit-prototype",
            any(
                all(target_arch = "aarch64", target_os = "macos"),
                all(target_arch = "x86_64", target_os = "linux")
            )
        ))]
        {
            let telemetry = crate::jit::telemetry();
            let _ = writeln!(err, "jit_runtime_enabled={}", u8::from(telemetry.enabled));
            let _ = writeln!(
                err,
                "jit_code_mapping_limit_bytes={}",
                telemetry.mapping_limit_bytes
            );
            let _ = writeln!(
                err,
                "jit_code_mapping_live_bytes={}",
                telemetry.live_mapping_bytes
            );
            let _ = writeln!(
                err,
                "jit_code_mapping_peak_bytes={}",
                telemetry.peak_mapping_bytes
            );
            let _ = writeln!(
                err,
                "jit_code_mapping_live_count={}",
                telemetry.live_mappings
            );
            let _ = writeln!(
                err,
                "jit_code_mapping_peak_count={}",
                telemetry.peak_mappings
            );
            let _ = writeln!(
                err,
                "jit_code_mapping_created_count={}",
                telemetry.created_mappings
            );
            let _ = writeln!(
                err,
                "jit_code_mapping_disabled_rejections={}",
                telemetry.disabled_rejections
            );
            let _ = writeln!(
                err,
                "jit_code_mapping_budget_rejections={}",
                telemetry.budget_rejections
            );
            let _ = writeln!(
                err,
                "jit_code_mapping_system_failures={}",
                telemetry.system_failures
            );
        }
        let _ = writeln!(err, "-- quick/JIT planner coverage --");
        let _ = writeln!(
            err,
            "jit_loop_candidates={}",
            JIT_LOOP_CANDIDATES.load(Ordering::Relaxed)
        );
        let loop_admissions = JIT_LOOP_ADMISSIONS
            .iter()
            .map(|counter| counter.load(Ordering::Relaxed))
            .sum::<u64>();
        let loop_rejections = JIT_LOOP_REJECTIONS
            .iter()
            .map(|counter| counter.load(Ordering::Relaxed))
            .sum::<u64>();
        let _ = writeln!(err, "jit_loop_admissions={loop_admissions}");
        let _ = writeln!(err, "jit_loop_rejections={loop_rejections}");
        let _ = writeln!(
            err,
            "jit_straight_candidates={}",
            JIT_STRAIGHT_CANDIDATES.load(Ordering::Relaxed)
        );
        let _ = writeln!(
            err,
            "jit_straight_admissions={}",
            JIT_STRAIGHT_ADMISSIONS.load(Ordering::Relaxed)
        );
        let _ = writeln!(err, "-- rejected straight regions by admission stage --");
        for (index, counter) in JIT_STRAIGHT_REJECTIONS.iter().enumerate() {
            let count = counter.load(Ordering::Relaxed);
            if count > 0 {
                let _ = writeln!(err, "{}={count}", jit_straight_miss_name(index));
            }
        }
        let _ = writeln!(err, "-- admitted regions by shape --");
        for (index, counter) in JIT_LOOP_ADMISSIONS.iter().enumerate() {
            let count = counter.load(Ordering::Relaxed);
            if count > 0 {
                let _ = writeln!(err, "{}={count}", jit_region_name(index));
            }
        }
        let straight_admissions = JIT_STRAIGHT_ADMISSIONS.load(Ordering::Relaxed);
        if straight_admissions > 0 {
            let _ = writeln!(err, "straight_array_region={straight_admissions}");
        }
        let _ = writeln!(err, "-- executed optimized regions by shape --");
        for (index, counter) in JIT_REGION_EXECUTIONS.iter().enumerate() {
            let count = counter.load(Ordering::Relaxed);
            if count > 0 {
                let _ = writeln!(err, "{}={count}", jit_region_name(index));
            }
        }
        let _ = writeln!(err, "-- native JIT executions by shape --");
        for (index, counter) in JIT_NATIVE_EXECUTIONS.iter().enumerate() {
            let count = counter.load(Ordering::Relaxed);
            if count > 0 {
                let side_exits = JIT_NATIVE_SIDE_EXITS[index].load(Ordering::Relaxed);
                let _ = writeln!(
                    err,
                    "{}={count},side_exits={side_exits}",
                    jit_region_name(index)
                );
            }
        }
        let _ = writeln!(err, "-- rejected loops by dominant gap --");
        for (index, counter) in JIT_LOOP_REJECTIONS.iter().enumerate() {
            let count = counter.load(Ordering::Relaxed);
            if count > 0 {
                let _ = writeln!(err, "{}={count}", jit_miss_name(index));
            }
        }
        let _ = writeln!(err, "-- rejected backedge executions by dominant gap --");
        for (index, counter) in JIT_REJECTED_BACKEDGE_HITS.iter().enumerate() {
            let count = counter.load(Ordering::Relaxed);
            if count > 0 {
                let _ = writeln!(err, "{}={count}", jit_miss_name(index));
            }
        }
        let _ = writeln!(
            err,
            "find_function_calls={}",
            FIND_FUNCTION_CALLS.load(Ordering::Relaxed)
        );
        let _ = writeln!(
            err,
            "find_function_exact_hits={}",
            FIND_FUNCTION_EXACT_HITS.load(Ordering::Relaxed)
        );
        let _ = writeln!(
            err,
            "find_function_lower_hits={}",
            FIND_FUNCTION_LOWER_HITS.load(Ordering::Relaxed)
        );
        let _ = writeln!(
            err,
            "find_function_misses={}",
            FIND_FUNCTION_MISSES.load(Ordering::Relaxed)
        );

        let _ = writeln!(err, "-- value.clone by type --");
        for (idx, counter) in VALUE_CLONES.iter().enumerate() {
            let count = counter.load(Ordering::Relaxed);
            if count > 0 {
                let _ = writeln!(err, "{}={}", value_kind_name(idx), count);
            }
        }

        let _ = writeln!(err, "-- value.drop by type --");
        for (idx, counter) in VALUE_DROPS.iter().enumerate() {
            let count = counter.load(Ordering::Relaxed);
            if count > 0 {
                let _ = writeln!(err, "{}={}", value_kind_name(idx), count);
            }
        }
        let _ = writeln!(err, "-- array ownership --");
        let _ = writeln!(
            err,
            "array_owner_allocations={}",
            ARRAY_OWNER_ALLOCATIONS.load(Ordering::Relaxed)
        );
        let _ = writeln!(err, "-- closure ownership --");
        let _ = writeln!(
            err,
            "closure_payload_allocations={}",
            CLOSURE_PAYLOAD_ALLOCATIONS.load(Ordering::Relaxed)
        );
        let _ = writeln!(
            err,
            "closure_capture_storage_allocations={}",
            CLOSURE_CAPTURE_STORAGE_ALLOCATIONS.load(Ordering::Relaxed)
        );
        let _ = writeln!(err, "-- declared object lifecycle --");
        let _ = writeln!(
            err,
            "declared_object_owner_allocations={}",
            DECLARED_OBJECT_OWNER_ALLOCATIONS.load(Ordering::Relaxed)
        );
        let _ = writeln!(
            err,
            "declared_property_storage_allocations={}",
            DECLARED_PROPERTY_STORAGE_ALLOCATIONS.load(Ordering::Relaxed)
        );
        let _ = writeln!(
            err,
            "declared_property_storage_reuses={}",
            DECLARED_PROPERTY_STORAGE_REUSES.load(Ordering::Relaxed)
        );
        let _ = writeln!(
            err,
            "declared_property_storage_returns={}",
            DECLARED_PROPERTY_STORAGE_RETURNS.load(Ordering::Relaxed)
        );
        let _ = writeln!(err, "-- new object resolution --");
        let _ = writeln!(
            err,
            "newobj_literal_cache_hits={}",
            NEWOBJ_LITERAL_CACHE_HITS.load(Ordering::Relaxed)
        );
        let _ = writeln!(
            err,
            "newobj_literal_cache_misses={}",
            NEWOBJ_LITERAL_CACHE_MISSES.load(Ordering::Relaxed)
        );
        let _ = writeln!(
            err,
            "newobj_class_name_materializations={}",
            NEWOBJ_CLASS_NAME_MATERIALIZATIONS.load(Ordering::Relaxed)
        );
        let _ = writeln!(
            err,
            "newobj_class_hash_lookups={}",
            NEWOBJ_CLASS_HASH_LOOKUPS.load(Ordering::Relaxed)
        );
        let _ = writeln!(err, "-- resolved virtual aggregate --");
        let _ = writeln!(
            err,
            "resolved_virtual_aggregate_resolve_attempts={}",
            RESOLVED_VIRTUAL_AGGREGATE_RESOLVE_ATTEMPTS.load(Ordering::Relaxed)
        );
        let _ = writeln!(
            err,
            "resolved_virtual_aggregate_resolve_successes={}",
            RESOLVED_VIRTUAL_AGGREGATE_RESOLVE_SUCCESSES.load(Ordering::Relaxed)
        );
        let _ = writeln!(
            err,
            "resolved_virtual_aggregate_cache_hits={}",
            RESOLVED_VIRTUAL_AGGREGATE_CACHE_HITS.load(Ordering::Relaxed)
        );
        let _ = writeln!(
            err,
            "resolved_virtual_aggregate_cache_invalidations={}",
            RESOLVED_VIRTUAL_AGGREGATE_CACHE_INVALIDATIONS.load(Ordering::Relaxed)
        );
        let _ = writeln!(
            err,
            "resolved_virtual_aggregate_guard_fallbacks={}",
            RESOLVED_VIRTUAL_AGGREGATE_GUARD_FALLBACKS.load(Ordering::Relaxed)
        );

        let mut opcodes = Vec::new();
        for (idx, counter) in OPCODE_COUNTS.iter().enumerate() {
            let count = counter.load(Ordering::Relaxed);
            if count > 0 {
                opcodes.push((idx, count));
            }
        }
        opcodes.sort_by(|a, b| b.1.cmp(&a.1));

        let _ = writeln!(err, "-- opcode counts --");
        for (idx, count) in opcodes {
            if let Some(name) = opcode_name(idx) {
                let _ = writeln!(err, "{}={}", name, count);
            } else {
                let _ = writeln!(err, "opcode_{}={}", idx, count);
            }
        }
    }
}

// ── Public API: delegates to inner when feature is enabled, no-ops otherwise ──

#[cfg(feature = "vm-stats")]
#[inline(always)]
pub fn configure_from_env() {
    inner::configure_from_env();
}
#[cfg(not(feature = "vm-stats"))]
#[inline(always)]
pub fn configure_from_env() {}

#[cfg(feature = "vm-stats")]
#[inline(always)]
pub fn enabled() -> bool {
    inner::enabled()
}
#[cfg(not(feature = "vm-stats"))]
#[inline(always)]
pub fn enabled() -> bool {
    false
}

#[cfg(feature = "vm-stats")]
#[inline(always)]
pub fn reset() {
    inner::reset();
}
#[cfg(not(feature = "vm-stats"))]
#[inline(always)]
pub fn reset() {}

#[cfg(feature = "vm-stats")]
#[inline(always)]
pub fn inc_push_call_frame(slot_count: usize, zero_bytes: usize) {
    inner::inc_push_call_frame(slot_count, zero_bytes);
}
#[cfg(not(feature = "vm-stats"))]
#[inline(always)]
pub fn inc_push_call_frame(_slot_count: usize, _zero_bytes: usize) {}

#[cfg(feature = "vm-stats")]
#[inline(always)]
pub fn inc_cleanup_frame(slot_count: usize, skipped: bool) {
    inner::inc_cleanup_frame(slot_count, skipped);
}
#[cfg(not(feature = "vm-stats"))]
#[inline(always)]
pub fn inc_cleanup_frame(_slot_count: usize, _skipped: bool) {}

#[cfg(feature = "vm-stats")]
#[inline(always)]
pub fn inc_write_val() {
    inner::inc_write_val();
}
#[cfg(not(feature = "vm-stats"))]
#[inline(always)]
pub fn inc_write_val() {}

#[cfg(feature = "vm-stats")]
#[inline(always)]
pub fn inc_write_frame_slot(heap_value: bool) {
    inner::inc_write_frame_slot(heap_value);
}
#[cfg(not(feature = "vm-stats"))]
#[inline(always)]
pub fn inc_write_frame_slot(_heap_value: bool) {}

#[cfg(feature = "vm-stats")]
#[inline(always)]
pub fn inc_do_fcall_fast() {
    inner::inc_do_fcall_fast();
}
#[cfg(not(feature = "vm-stats"))]
#[inline(always)]
pub fn inc_do_fcall_fast() {}

#[cfg(feature = "vm-stats")]
#[inline(always)]
pub fn inc_do_fcall_fast_by(count: u64) {
    inner::inc_do_fcall_fast_by(count);
}
#[cfg(not(feature = "vm-stats"))]
#[inline(always)]
pub fn inc_do_fcall_fast_by(_count: u64) {}

#[cfg(feature = "vm-stats")]
#[inline(always)]
pub fn inc_do_fcall_full() {
    inner::inc_do_fcall_full();
}
#[cfg(not(feature = "vm-stats"))]
#[inline(always)]
pub fn inc_do_fcall_full() {}

#[cfg(feature = "vm-stats")]
#[inline(always)]
pub fn inc_return_fast() {
    inner::inc_return_fast();
}
#[cfg(not(feature = "vm-stats"))]
#[inline(always)]
pub fn inc_return_fast() {}

#[cfg(feature = "vm-stats")]
#[inline(always)]
pub fn inc_return_fast_by(count: u64) {
    inner::inc_return_fast_by(count);
}
#[cfg(not(feature = "vm-stats"))]
#[inline(always)]
pub fn inc_return_fast_by(_count: u64) {}

#[cfg(feature = "vm-stats")]
#[inline(always)]
pub fn inc_return_full() {
    inner::inc_return_full();
}
#[cfg(not(feature = "vm-stats"))]
#[inline(always)]
pub fn inc_return_full() {}

#[cfg(feature = "vm-stats")]
#[inline(always)]
pub fn inc_quick_loop_completed(iterations: u64) {
    inner::inc_quick_loop_completed(iterations);
}
#[cfg(not(feature = "vm-stats"))]
#[inline(always)]
pub fn inc_quick_loop_completed(_iterations: u64) {}

#[cfg(feature = "vm-stats")]
#[inline(always)]
pub fn inc_quick_loop_deoptimized(iterations: u64) {
    inner::inc_quick_loop_deoptimized(iterations);
}
#[cfg(not(feature = "vm-stats"))]
#[inline(always)]
pub fn inc_quick_loop_deoptimized(_iterations: u64) {}

#[cfg(feature = "vm-stats")]
#[inline(always)]
pub fn inc_quick_loop_guard_failed() {
    inner::inc_quick_loop_guard_failed();
}
#[cfg(not(feature = "vm-stats"))]
#[inline(always)]
pub fn inc_quick_loop_guard_failed() {}

#[cfg(feature = "vm-stats")]
#[inline(always)]
pub fn record_quick_packed_array_reserve(entries: usize, succeeded: bool) {
    inner::record_quick_packed_array_reserve(entries, succeeded);
}
#[cfg(not(feature = "vm-stats"))]
#[inline(always)]
pub fn record_quick_packed_array_reserve(_entries: usize, _succeeded: bool) {}

#[cfg(feature = "vm-stats")]
#[inline(always)]
pub fn inc_jit_loop_candidate() {
    inner::inc_jit_loop_candidate();
}
#[cfg(not(feature = "vm-stats"))]
#[inline(always)]
pub fn inc_jit_loop_candidate() {}

#[cfg(feature = "vm-stats")]
#[inline(always)]
pub fn inc_jit_loop_admitted(kind: JitRegionKind) {
    inner::inc_jit_loop_admitted(kind);
}
#[cfg(not(feature = "vm-stats"))]
#[inline(always)]
pub fn inc_jit_loop_admitted(_kind: JitRegionKind) {}

#[cfg(feature = "vm-stats")]
#[inline(always)]
pub fn inc_jit_loop_rejected(reason: JitMissReason) {
    inner::inc_jit_loop_rejected(reason);
}
#[cfg(not(feature = "vm-stats"))]
#[inline(always)]
pub fn inc_jit_loop_rejected(_reason: JitMissReason) {}

#[cfg(feature = "vm-stats")]
#[inline(always)]
pub fn inc_jit_rejected_backedge_hit(marker: u32) {
    inner::inc_jit_rejected_backedge_hit(marker);
}
#[cfg(not(feature = "vm-stats"))]
#[inline(always)]
pub fn inc_jit_rejected_backedge_hit(_marker: u32) {}

#[cfg(feature = "vm-stats")]
#[inline(always)]
pub fn inc_jit_region_execution(kind: JitRegionKind) {
    inner::inc_jit_region_execution(kind);
}
#[cfg(not(feature = "vm-stats"))]
#[inline(always)]
pub fn inc_jit_region_execution(_kind: JitRegionKind) {}

#[cfg(feature = "vm-stats")]
#[inline(always)]
pub fn inc_jit_native_execution(kind: JitRegionKind) {
    inner::inc_jit_native_execution(kind);
}
#[cfg(not(feature = "vm-stats"))]
#[inline(always)]
pub fn inc_jit_native_execution(_kind: JitRegionKind) {}

#[cfg(feature = "vm-stats")]
#[inline(always)]
pub fn inc_jit_native_side_exit(kind: JitRegionKind) {
    inner::inc_jit_native_side_exit(kind);
}
#[cfg(not(feature = "vm-stats"))]
#[inline(always)]
pub fn inc_jit_native_side_exit(_kind: JitRegionKind) {}

#[cfg(feature = "vm-stats")]
#[inline(always)]
pub fn inc_jit_straight_candidate() {
    inner::inc_jit_straight_candidate();
}
#[cfg(not(feature = "vm-stats"))]
#[inline(always)]
pub fn inc_jit_straight_candidate() {}

#[cfg(feature = "vm-stats")]
#[inline(always)]
pub fn inc_jit_straight_admitted() {
    inner::inc_jit_straight_admitted();
}
#[cfg(not(feature = "vm-stats"))]
#[inline(always)]
pub fn inc_jit_straight_admitted() {}

#[cfg(feature = "vm-stats")]
#[inline(always)]
pub fn inc_jit_straight_rejected(reason: JitStraightMissReason) {
    inner::inc_jit_straight_rejected(reason);
}
#[cfg(not(feature = "vm-stats"))]
#[inline(always)]
pub fn inc_jit_straight_rejected(_reason: JitStraightMissReason) {}

#[cfg(feature = "vm-stats")]
#[inline(always)]
pub fn inc_find_function_exact_hit() {
    inner::inc_find_function_exact_hit();
}
#[cfg(not(feature = "vm-stats"))]
#[inline(always)]
pub fn inc_find_function_exact_hit() {}

#[cfg(feature = "vm-stats")]
#[inline(always)]
pub fn inc_find_function_lower_hit() {
    inner::inc_find_function_lower_hit();
}
#[cfg(not(feature = "vm-stats"))]
#[inline(always)]
pub fn inc_find_function_lower_hit() {}

#[cfg(feature = "vm-stats")]
#[inline(always)]
pub fn inc_find_function_miss() {
    inner::inc_find_function_miss();
}
#[cfg(not(feature = "vm-stats"))]
#[inline(always)]
pub fn inc_find_function_miss() {}

#[cfg(feature = "vm-stats")]
#[inline(always)]
pub fn inc_value_clone(kind: usize) {
    inner::inc_value_clone(kind);
}
#[cfg(not(feature = "vm-stats"))]
#[inline(always)]
pub fn inc_value_clone(_kind: usize) {}

#[cfg(feature = "vm-stats")]
#[inline(always)]
pub fn inc_value_drop(kind: usize) {
    inner::inc_value_drop(kind);
}
#[cfg(not(feature = "vm-stats"))]
#[inline(always)]
pub fn inc_value_drop(_kind: usize) {}

#[cfg(feature = "vm-stats")]
#[inline(always)]
pub fn inc_array_owner_allocation() {
    inner::inc_array_owner_allocation();
}
#[cfg(not(feature = "vm-stats"))]
#[inline(always)]
pub fn inc_array_owner_allocation() {}

#[cfg(feature = "vm-stats")]
#[inline(always)]
pub fn inc_closure_payload_allocation() {
    inner::inc_closure_payload_allocation();
}
#[cfg(not(feature = "vm-stats"))]
#[inline(always)]
pub fn inc_closure_payload_allocation() {}

#[cfg(feature = "vm-stats")]
#[inline(always)]
pub fn inc_closure_capture_storage_allocation() {
    inner::inc_closure_capture_storage_allocation();
}
#[cfg(not(feature = "vm-stats"))]
#[inline(always)]
pub fn inc_closure_capture_storage_allocation() {}

#[cfg(feature = "vm-stats")]
#[inline(always)]
pub fn inc_declared_object_owner_allocation() {
    inner::inc_declared_object_owner_allocation();
}
#[cfg(not(feature = "vm-stats"))]
#[inline(always)]
pub fn inc_declared_object_owner_allocation() {}

#[cfg(feature = "vm-stats")]
#[inline(always)]
pub fn inc_declared_property_storage_allocation() {
    inner::inc_declared_property_storage_allocation();
}
#[cfg(not(feature = "vm-stats"))]
#[inline(always)]
pub fn inc_declared_property_storage_allocation() {}

#[cfg(feature = "vm-stats")]
#[inline(always)]
pub fn inc_declared_property_storage_reuse() {
    inner::inc_declared_property_storage_reuse();
}
#[cfg(not(feature = "vm-stats"))]
#[inline(always)]
pub fn inc_declared_property_storage_reuse() {}

#[cfg(feature = "vm-stats")]
#[inline(always)]
pub fn inc_declared_property_storage_return() {
    inner::inc_declared_property_storage_return();
}
#[cfg(not(feature = "vm-stats"))]
#[inline(always)]
pub fn inc_declared_property_storage_return() {}

#[cfg(feature = "vm-stats")]
#[inline(always)]
pub fn inc_newobj_literal_cache_hit() {
    inner::inc_newobj_literal_cache_hit();
}
#[cfg(not(feature = "vm-stats"))]
#[inline(always)]
pub fn inc_newobj_literal_cache_hit() {}

#[cfg(feature = "vm-stats")]
#[inline(always)]
pub fn inc_newobj_literal_cache_miss() {
    inner::inc_newobj_literal_cache_miss();
}
#[cfg(not(feature = "vm-stats"))]
#[inline(always)]
pub fn inc_newobj_literal_cache_miss() {}

#[cfg(feature = "vm-stats")]
#[inline(always)]
pub fn inc_newobj_class_name_materialization() {
    inner::inc_newobj_class_name_materialization();
}
#[cfg(not(feature = "vm-stats"))]
#[inline(always)]
pub fn inc_newobj_class_name_materialization() {}

#[cfg(feature = "vm-stats")]
#[inline(always)]
pub fn inc_newobj_class_hash_lookup() {
    inner::inc_newobj_class_hash_lookup();
}
#[cfg(not(feature = "vm-stats"))]
#[inline(always)]
pub fn inc_newobj_class_hash_lookup() {}

#[cfg(feature = "vm-stats")]
#[inline(always)]
pub fn inc_resolved_virtual_aggregate_resolve_attempt() {
    inner::inc_resolved_virtual_aggregate_resolve_attempt();
}
#[cfg(not(feature = "vm-stats"))]
#[inline(always)]
pub fn inc_resolved_virtual_aggregate_resolve_attempt() {}

#[cfg(feature = "vm-stats")]
#[inline(always)]
pub fn inc_resolved_virtual_aggregate_resolve_success() {
    inner::inc_resolved_virtual_aggregate_resolve_success();
}
#[cfg(not(feature = "vm-stats"))]
#[inline(always)]
pub fn inc_resolved_virtual_aggregate_resolve_success() {}

#[cfg(feature = "vm-stats")]
#[inline(always)]
pub fn inc_resolved_virtual_aggregate_cache_hit() {
    inner::inc_resolved_virtual_aggregate_cache_hit();
}
#[cfg(not(feature = "vm-stats"))]
#[inline(always)]
pub fn inc_resolved_virtual_aggregate_cache_hit() {}

#[cfg(feature = "vm-stats")]
#[inline(always)]
pub fn inc_resolved_virtual_aggregate_cache_invalidation() {
    inner::inc_resolved_virtual_aggregate_cache_invalidation();
}
#[cfg(not(feature = "vm-stats"))]
#[inline(always)]
pub fn inc_resolved_virtual_aggregate_cache_invalidation() {}

#[cfg(feature = "vm-stats")]
#[inline(always)]
pub fn inc_resolved_virtual_aggregate_guard_fallback() {
    inner::inc_resolved_virtual_aggregate_guard_fallback();
}
#[cfg(not(feature = "vm-stats"))]
#[inline(always)]
pub fn inc_resolved_virtual_aggregate_guard_fallback() {}

#[cfg(feature = "vm-stats")]
#[inline(always)]
pub fn inc_opcode(opcode: usize) {
    inner::inc_opcode(opcode);
}
#[cfg(not(feature = "vm-stats"))]
#[inline(always)]
pub fn inc_opcode(_opcode: usize) {}

#[cfg(feature = "vm-stats")]
#[inline(always)]
pub fn dump_to_stderr() {
    inner::dump_to_stderr();
}
#[cfg(not(feature = "vm-stats"))]
#[inline(always)]
pub fn dump_to_stderr() {}
