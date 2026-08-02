// VM runtime statistics — compile-time gated behind `vm-stats` feature.
// Without the feature, all functions compile to nothing (zero overhead).
// Usage: cargo build --features vm-stats && RPHP_VM_STATS=1 ./target/release/rphp script.php

#[cfg(feature = "vm-stats")]
mod inner {
    use std::io::Write;
    use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};

    const VALUE_KIND_COUNT: usize = 11;
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

    static FIND_FUNCTION_CALLS: AtomicU64 = AtomicU64::new(0);
    static FIND_FUNCTION_EXACT_HITS: AtomicU64 = AtomicU64::new(0);
    static FIND_FUNCTION_LOWER_HITS: AtomicU64 = AtomicU64::new(0);
    static FIND_FUNCTION_MISSES: AtomicU64 = AtomicU64::new(0);

    static VALUE_CLONES: [AtomicU64; VALUE_KIND_COUNT] =
        [const { AtomicU64::new(0) }; VALUE_KIND_COUNT];
    static VALUE_DROPS: [AtomicU64; VALUE_KIND_COUNT] =
        [const { AtomicU64::new(0) }; VALUE_KIND_COUNT];
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
        for counter in &OPCODE_COUNTS {
            counter.store(0, Ordering::Relaxed);
        }
    }

    #[inline]
    pub fn inc_push_call_frame(slot_count: usize, zero_bytes: usize) {
        if !enabled() { return; }
        PUSH_CALL_FRAME_CALLS.fetch_add(1, Ordering::Relaxed);
        PUSH_CALL_FRAME_ZERO_SLOTS.fetch_add(slot_count as u64, Ordering::Relaxed);
        PUSH_CALL_FRAME_ZERO_BYTES.fetch_add(zero_bytes as u64, Ordering::Relaxed);
    }

    #[inline]
    pub fn inc_cleanup_frame(slot_count: usize, skipped: bool) {
        if !enabled() { return; }
        CLEANUP_FRAME_CALLS.fetch_add(1, Ordering::Relaxed);
        if skipped {
            CLEANUP_FRAME_FAST_SKIPS.fetch_add(1, Ordering::Relaxed);
        } else {
            CLEANUP_FRAME_SCANNED_SLOTS.fetch_add(slot_count as u64, Ordering::Relaxed);
        }
    }

    #[inline]
    pub fn inc_write_val() {
        if enabled() { WRITE_VAL_CALLS.fetch_add(1, Ordering::Relaxed); }
    }

    #[inline]
    pub fn inc_write_frame_slot(heap_value: bool) {
        if !enabled() { return; }
        WRITE_FRAME_SLOT_CALLS.fetch_add(1, Ordering::Relaxed);
        if heap_value { WRITE_FRAME_SLOT_HEAP_VALUES.fetch_add(1, Ordering::Relaxed); }
    }

    #[inline]
    pub fn inc_do_fcall_fast() {
        if enabled() { DO_FCALL_FAST_PATHS.fetch_add(1, Ordering::Relaxed); }
    }

    #[inline]
    pub fn inc_do_fcall_fast_by(count: u64) {
        if enabled() { DO_FCALL_FAST_PATHS.fetch_add(count, Ordering::Relaxed); }
    }

    #[inline]
    pub fn inc_do_fcall_full() {
        if enabled() { DO_FCALL_FULL_PATHS.fetch_add(1, Ordering::Relaxed); }
    }

    #[inline]
    pub fn inc_return_fast() {
        if enabled() { RETURN_FAST_PATHS.fetch_add(1, Ordering::Relaxed); }
    }

    #[inline]
    pub fn inc_return_fast_by(count: u64) {
        if enabled() { RETURN_FAST_PATHS.fetch_add(count, Ordering::Relaxed); }
    }

    #[inline]
    pub fn inc_return_full() {
        if enabled() { RETURN_FULL_PATHS.fetch_add(1, Ordering::Relaxed); }
    }

    #[inline]
    pub fn inc_quick_loop_completed(iterations: u64) {
        if !enabled() { return; }
        QUICK_LOOP_ENTRIES.fetch_add(1, Ordering::Relaxed);
        QUICK_LOOP_COMPLETIONS.fetch_add(1, Ordering::Relaxed);
        QUICK_LOOP_ITERATIONS.fetch_add(iterations, Ordering::Relaxed);
    }

    #[inline]
    pub fn inc_quick_loop_deoptimized(iterations: u64) {
        if !enabled() { return; }
        QUICK_LOOP_ENTRIES.fetch_add(1, Ordering::Relaxed);
        QUICK_LOOP_DEOPTIMIZATIONS.fetch_add(1, Ordering::Relaxed);
        QUICK_LOOP_ITERATIONS.fetch_add(iterations, Ordering::Relaxed);
    }

    #[inline]
    pub fn inc_quick_loop_guard_failed() {
        if !enabled() { return; }
        QUICK_LOOP_GUARD_FAILURES.fetch_add(1, Ordering::Relaxed);
    }

    #[inline]
    pub fn inc_find_function_exact_hit() {
        if !enabled() { return; }
        FIND_FUNCTION_CALLS.fetch_add(1, Ordering::Relaxed);
        FIND_FUNCTION_EXACT_HITS.fetch_add(1, Ordering::Relaxed);
    }

    #[inline]
    pub fn inc_find_function_lower_hit() {
        if !enabled() { return; }
        FIND_FUNCTION_CALLS.fetch_add(1, Ordering::Relaxed);
        FIND_FUNCTION_LOWER_HITS.fetch_add(1, Ordering::Relaxed);
    }

    #[inline]
    pub fn inc_find_function_miss() {
        if !enabled() { return; }
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
    pub fn inc_opcode(opcode: usize) {
        if enabled() && opcode < OPCODE_KIND_COUNT {
            OPCODE_COUNTS[opcode].fetch_add(1, Ordering::Relaxed);
        }
    }

    fn value_kind_name(kind: usize) -> &'static str {
        match kind {
            0 => "undef", 1 => "null", 2 => "false", 3 => "true",
            4 => "long", 5 => "double", 6 => "string", 7 => "array",
            8 => "object", 9 => "resource", 10 => "reference",
            _ => "unknown",
        }
    }

    fn opcode_name(op: usize) -> Option<&'static str> {
        match op {
            1 => Some("Add"), 2 => Some("Sub"), 3 => Some("Mul"),
            4 => Some("Div"), 5 => Some("Mod"), 8 => Some("Concat"),
            9 => Some("AssignConcat"),
            10 => Some("AssignCv"), 13 => Some("BoolNot"),
            15 => Some("IsEqual"), 16 => Some("IsNotEqual"),
            17 => Some("IsSmaller"), 18 => Some("IsSmallerOrEqual"),
            19 => Some("IsIdentical"), 20 => Some("IsNotIdentical"),
            21 => Some("Isset"), 22 => Some("Cast"),
            34 => Some("PreInc"), 35 => Some("PreDec"),
            36 => Some("PostInc"), 37 => Some("PostDec"),
            40 => Some("Echo"), 42 => Some("Jmp"),
            43 => Some("JmpZ"), 44 => Some("JmpNZ"),
            60 => Some("DoFcall"), 61 => Some("InitFcall"),
            62 => Some("Return"), 63 => Some("SendVal"),
            64 => Some("SendRef"), 65 => Some("SendVarEx"),
            66 => Some("SendNamed"),
            67 => Some("CallUserFuncArray"),
            68 => Some("InitUserCall"), 69 => Some("SendUser"),
            70 => Some("InitArray"), 71 => Some("AddArrayElement"),
            72 => Some("FetchDimR"), 73 => Some("AssignDim"),
            74 => Some("ArrayPushOp"), 75 => Some("UnsetDim"),
            80 => Some("ForeachInit"), 81 => Some("ForeachNext"),
            90 => Some("Throw"),
            100 => Some("NewObj"), 101 => Some("FetchObjR"),
            102 => Some("AssignObjProp"), 103 => Some("InitMethodCall"),
            104 => Some("FetchStaticProp"), 105 => Some("InitStaticCall"),
            106 => Some("InitDynamicCall"), 107 => Some("Instanceof"),
            108 => Some("FetchConst"), 109 => Some("BindDefaultParam"),
            110 => Some("Yield"), 111 => Some("YieldFrom"),
            112 => Some("GeneratorReturn"), 113 => Some("Spaceship"),
            114 => Some("Pow"), 115 => Some("BitwiseAnd"),
            116 => Some("BitwiseOr"), 117 => Some("BitwiseXor"),
            118 => Some("ShiftLeft"), 119 => Some("ShiftRight"),
            120 => Some("BitwiseNot"), 121 => Some("BindGlobal"),
            123 => Some("BindStatic"), 124 => Some("AssignObjDim"),
            125 => Some("Include"), 126 => Some("NullSafeCheck"),
            127 => Some("CloneObj"),
            128 => Some("CreateClosure"), 129 => Some("ClosureUseVar"),
            130 => Some("DirectInternalCall1"),
            131 => Some("Strlen"),
            132 => Some("DirectInternalCall2"),
            200 => Some("Add_TmpTmp"), 201 => Some("Sub_CvConst"),
            202 => Some("IsSmaller_CvConst"), 203 => Some("IsSmallerOrEqual_CvConst"),
            204 => Some("Add_CvTmp"), 205 => Some("Sub_TmpTmp"),
            206 => Some("JmpZ_Le_CvConst"), 207 => Some("JmpNZ_Le_CvConst"),
            208 => Some("JmpZ_Lt_CvConst"), 209 => Some("JmpNZ_Lt_CvConst"),
            210 => Some("IsEqual_CvConst"),
            211 => Some("JmpZ_Eq_CvConst"), 212 => Some("JmpNZ_Eq_CvConst"),
            213 => Some("QuickLongLoopJmp"),
            214 => Some("Strlen_Cv"),
            _ => None,
        }
    }

    pub fn dump_to_stderr() {
        if !enabled() { return; }

        let mut err = std::io::stderr().lock();
        let _ = writeln!(err, "\n=== RPHP VM Stats ===");
        let _ = writeln!(err, "push_call_frame_calls={}", PUSH_CALL_FRAME_CALLS.load(Ordering::Relaxed));
        let _ = writeln!(err, "push_call_frame_zero_slots={}", PUSH_CALL_FRAME_ZERO_SLOTS.load(Ordering::Relaxed));
        let _ = writeln!(err, "push_call_frame_zero_bytes={}", PUSH_CALL_FRAME_ZERO_BYTES.load(Ordering::Relaxed));
        let _ = writeln!(err, "cleanup_frame_calls={}", CLEANUP_FRAME_CALLS.load(Ordering::Relaxed));
        let _ = writeln!(err, "cleanup_frame_fast_skips={}", CLEANUP_FRAME_FAST_SKIPS.load(Ordering::Relaxed));
        let _ = writeln!(err, "cleanup_frame_scanned_slots={}", CLEANUP_FRAME_SCANNED_SLOTS.load(Ordering::Relaxed));
        let _ = writeln!(err, "write_val_calls={}", WRITE_VAL_CALLS.load(Ordering::Relaxed));
        let _ = writeln!(err, "write_frame_slot_calls={}", WRITE_FRAME_SLOT_CALLS.load(Ordering::Relaxed));
        let _ = writeln!(err, "write_frame_slot_heap_values={}", WRITE_FRAME_SLOT_HEAP_VALUES.load(Ordering::Relaxed));
        let _ = writeln!(err, "do_fcall_fast_paths={}", DO_FCALL_FAST_PATHS.load(Ordering::Relaxed));
        let _ = writeln!(err, "do_fcall_full_paths={}", DO_FCALL_FULL_PATHS.load(Ordering::Relaxed));
        let _ = writeln!(err, "return_fast_paths={}", RETURN_FAST_PATHS.load(Ordering::Relaxed));
        let _ = writeln!(err, "return_full_paths={}", RETURN_FULL_PATHS.load(Ordering::Relaxed));
        let _ = writeln!(err, "quick_loop_entries={}", QUICK_LOOP_ENTRIES.load(Ordering::Relaxed));
        let _ = writeln!(err, "quick_loop_completions={}", QUICK_LOOP_COMPLETIONS.load(Ordering::Relaxed));
        let _ = writeln!(err, "quick_loop_deoptimizations={}", QUICK_LOOP_DEOPTIMIZATIONS.load(Ordering::Relaxed));
        let _ = writeln!(err, "quick_loop_guard_failures={}", QUICK_LOOP_GUARD_FAILURES.load(Ordering::Relaxed));
        let _ = writeln!(err, "quick_loop_iterations={}", QUICK_LOOP_ITERATIONS.load(Ordering::Relaxed));
        let _ = writeln!(err, "find_function_calls={}", FIND_FUNCTION_CALLS.load(Ordering::Relaxed));
        let _ = writeln!(err, "find_function_exact_hits={}", FIND_FUNCTION_EXACT_HITS.load(Ordering::Relaxed));
        let _ = writeln!(err, "find_function_lower_hits={}", FIND_FUNCTION_LOWER_HITS.load(Ordering::Relaxed));
        let _ = writeln!(err, "find_function_misses={}", FIND_FUNCTION_MISSES.load(Ordering::Relaxed));

        let _ = writeln!(err, "-- value.clone by type --");
        for (idx, counter) in VALUE_CLONES.iter().enumerate() {
            let count = counter.load(Ordering::Relaxed);
            if count > 0 { let _ = writeln!(err, "{}={}", value_kind_name(idx), count); }
        }

        let _ = writeln!(err, "-- value.drop by type --");
        for (idx, counter) in VALUE_DROPS.iter().enumerate() {
            let count = counter.load(Ordering::Relaxed);
            if count > 0 { let _ = writeln!(err, "{}={}", value_kind_name(idx), count); }
        }

        let mut opcodes = Vec::new();
        for (idx, counter) in OPCODE_COUNTS.iter().enumerate() {
            let count = counter.load(Ordering::Relaxed);
            if count > 0 { opcodes.push((idx, count)); }
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
pub fn configure_from_env() { inner::configure_from_env(); }
#[cfg(not(feature = "vm-stats"))]
#[inline(always)]
pub fn configure_from_env() {}

#[cfg(feature = "vm-stats")]
#[inline(always)]
pub fn enabled() -> bool { inner::enabled() }
#[cfg(not(feature = "vm-stats"))]
#[inline(always)]
pub fn enabled() -> bool { false }

#[cfg(feature = "vm-stats")]
#[inline(always)]
pub fn reset() { inner::reset(); }
#[cfg(not(feature = "vm-stats"))]
#[inline(always)]
pub fn reset() {}

#[cfg(feature = "vm-stats")]
#[inline(always)]
pub fn inc_push_call_frame(slot_count: usize, zero_bytes: usize) { inner::inc_push_call_frame(slot_count, zero_bytes); }
#[cfg(not(feature = "vm-stats"))]
#[inline(always)]
pub fn inc_push_call_frame(_slot_count: usize, _zero_bytes: usize) {}

#[cfg(feature = "vm-stats")]
#[inline(always)]
pub fn inc_cleanup_frame(slot_count: usize, skipped: bool) { inner::inc_cleanup_frame(slot_count, skipped); }
#[cfg(not(feature = "vm-stats"))]
#[inline(always)]
pub fn inc_cleanup_frame(_slot_count: usize, _skipped: bool) {}

#[cfg(feature = "vm-stats")]
#[inline(always)]
pub fn inc_write_val() { inner::inc_write_val(); }
#[cfg(not(feature = "vm-stats"))]
#[inline(always)]
pub fn inc_write_val() {}

#[cfg(feature = "vm-stats")]
#[inline(always)]
pub fn inc_write_frame_slot(heap_value: bool) { inner::inc_write_frame_slot(heap_value); }
#[cfg(not(feature = "vm-stats"))]
#[inline(always)]
pub fn inc_write_frame_slot(_heap_value: bool) {}

#[cfg(feature = "vm-stats")]
#[inline(always)]
pub fn inc_do_fcall_fast() { inner::inc_do_fcall_fast(); }
#[cfg(not(feature = "vm-stats"))]
#[inline(always)]
pub fn inc_do_fcall_fast() {}

#[cfg(feature = "vm-stats")]
#[inline(always)]
pub fn inc_do_fcall_fast_by(count: u64) { inner::inc_do_fcall_fast_by(count); }
#[cfg(not(feature = "vm-stats"))]
#[inline(always)]
pub fn inc_do_fcall_fast_by(_count: u64) {}

#[cfg(feature = "vm-stats")]
#[inline(always)]
pub fn inc_do_fcall_full() { inner::inc_do_fcall_full(); }
#[cfg(not(feature = "vm-stats"))]
#[inline(always)]
pub fn inc_do_fcall_full() {}

#[cfg(feature = "vm-stats")]
#[inline(always)]
pub fn inc_return_fast() { inner::inc_return_fast(); }
#[cfg(not(feature = "vm-stats"))]
#[inline(always)]
pub fn inc_return_fast() {}

#[cfg(feature = "vm-stats")]
#[inline(always)]
pub fn inc_return_fast_by(count: u64) { inner::inc_return_fast_by(count); }
#[cfg(not(feature = "vm-stats"))]
#[inline(always)]
pub fn inc_return_fast_by(_count: u64) {}

#[cfg(feature = "vm-stats")]
#[inline(always)]
pub fn inc_return_full() { inner::inc_return_full(); }
#[cfg(not(feature = "vm-stats"))]
#[inline(always)]
pub fn inc_return_full() {}

#[cfg(feature = "vm-stats")]
#[inline(always)]
pub fn inc_quick_loop_completed(iterations: u64) { inner::inc_quick_loop_completed(iterations); }
#[cfg(not(feature = "vm-stats"))]
#[inline(always)]
pub fn inc_quick_loop_completed(_iterations: u64) {}

#[cfg(feature = "vm-stats")]
#[inline(always)]
pub fn inc_quick_loop_deoptimized(iterations: u64) { inner::inc_quick_loop_deoptimized(iterations); }
#[cfg(not(feature = "vm-stats"))]
#[inline(always)]
pub fn inc_quick_loop_deoptimized(_iterations: u64) {}

#[cfg(feature = "vm-stats")]
#[inline(always)]
pub fn inc_quick_loop_guard_failed() { inner::inc_quick_loop_guard_failed(); }
#[cfg(not(feature = "vm-stats"))]
#[inline(always)]
pub fn inc_quick_loop_guard_failed() {}

#[cfg(feature = "vm-stats")]
#[inline(always)]
pub fn inc_find_function_exact_hit() { inner::inc_find_function_exact_hit(); }
#[cfg(not(feature = "vm-stats"))]
#[inline(always)]
pub fn inc_find_function_exact_hit() {}

#[cfg(feature = "vm-stats")]
#[inline(always)]
pub fn inc_find_function_lower_hit() { inner::inc_find_function_lower_hit(); }
#[cfg(not(feature = "vm-stats"))]
#[inline(always)]
pub fn inc_find_function_lower_hit() {}

#[cfg(feature = "vm-stats")]
#[inline(always)]
pub fn inc_find_function_miss() { inner::inc_find_function_miss(); }
#[cfg(not(feature = "vm-stats"))]
#[inline(always)]
pub fn inc_find_function_miss() {}

#[cfg(feature = "vm-stats")]
#[inline(always)]
pub fn inc_value_clone(kind: usize) { inner::inc_value_clone(kind); }
#[cfg(not(feature = "vm-stats"))]
#[inline(always)]
pub fn inc_value_clone(_kind: usize) {}

#[cfg(feature = "vm-stats")]
#[inline(always)]
pub fn inc_value_drop(kind: usize) { inner::inc_value_drop(kind); }
#[cfg(not(feature = "vm-stats"))]
#[inline(always)]
pub fn inc_value_drop(_kind: usize) {}

#[cfg(feature = "vm-stats")]
#[inline(always)]
pub fn inc_opcode(opcode: usize) { inner::inc_opcode(opcode); }
#[cfg(not(feature = "vm-stats"))]
#[inline(always)]
pub fn inc_opcode(_opcode: usize) {}

#[cfg(feature = "vm-stats")]
#[inline(always)]
pub fn dump_to_stderr() { inner::dump_to_stderr(); }
#[cfg(not(feature = "vm-stats"))]
#[inline(always)]
pub fn dump_to_stderr() {}
