// Kept in the execute module through include! so this structural split does not change visibility or code generation.

#[derive(Clone, Copy)]
#[cfg(feature = "quick-loops")]
struct QuickLongConditionalKernel {
    header_lhs: u16,
    header_rhs: QuickLongOperand,
    header_condition_tmp: Option<u16>,
    add_lhs: u16,
    add_rhs: u16,
    add_result: u16,
    destination: u16,
    add_resume_ip: usize,
    post_value: u16,
    post_result: Option<u16>,
    post_resume_ip: usize,
    body_target: QuickLongTarget,
    exit_target: QuickLongTarget,
}

#[derive(Clone, Copy)]
#[cfg(feature = "quick-loops")]
enum QuickLongConditionalBody {
    LessThan {
        lhs: u16,
        rhs: QuickLongOperand,
        condition_tmp: Option<u16>,
    },
    ModuloEqual {
        value: u16,
        divisor: i64,
        result: u16,
        resume_ip: usize,
        lhs: u16,
        rhs: QuickLongOperand,
        condition_tmp: Option<u16>,
    },
}

#[derive(Clone, Copy)]
#[cfg(all(
    feature = "quick-loops",
    feature = "jit-prototype",
    any(
        all(target_arch = "aarch64", target_os = "macos"),
        all(target_arch = "x86_64", target_os = "linux")
    )
))]
struct NativeQuickLongStraightKernel {
    config: NativeStraightLongLoopConfig,
    header_condition_tmp: Option<u16>,
    body_target: QuickLongTarget,
    exit_target: QuickLongTarget,
    post_resume_ip: usize,
    operation_resume_ips: [usize; NATIVE_STRAIGHT_LONG_MAX_OPERATIONS],
    trace_guard_operation_indices: [u8; NATIVE_STRAIGHT_LONG_MAX_OPERATIONS],
    trace_guard_condition_slots: [u8; NATIVE_STRAIGHT_LONG_MAX_OPERATIONS],
    trace_guard_expected: [bool; NATIVE_STRAIGHT_LONG_MAX_OPERATIONS],
    trace_guard_count: u8,
    mutable_slots: [u8; NATIVE_QUICK_LONG_SLOT_CAPACITY],
    mutable_slot_count: u8,
}

#[cfg(all(
    feature = "quick-loops",
    feature = "jit-prototype",
    any(
        all(target_arch = "aarch64", target_os = "macos"),
        all(target_arch = "x86_64", target_os = "linux")
    )
))]
const NATIVE_FINITE_STRING_LIMIT: usize = 4;

#[cfg(all(
    feature = "quick-loops",
    feature = "jit-prototype",
    any(
        all(target_arch = "aarch64", target_os = "macos"),
        all(target_arch = "x86_64", target_os = "linux")
    )
))]
const NATIVE_QUICK_LONG_SLOT_CAPACITY: usize = 64;

#[derive(Clone, Copy)]
#[cfg(all(
    feature = "quick-loops",
    feature = "jit-prototype",
    any(
        all(target_arch = "aarch64", target_os = "macos"),
        all(target_arch = "x86_64", target_os = "linux")
    )
))]
struct NativeQuickLongMixedKernel {
    config: NativeStraightLongLoopConfig,
    header_condition_tmp: Option<u16>,
    body_target: QuickLongTarget,
    exit_target: QuickLongTarget,
    post_resume_ip: usize,
    operation_resume_ips: [usize; NATIVE_STRAIGHT_LONG_MAX_OPERATIONS],
    string_literals: [u16; NATIVE_FINITE_STRING_LIMIT],
    string_token_count: u8,
    context_array_slots: [u16; NATIVE_STRAIGHT_LONG_MAX_CONTEXT_ENTRIES],
    context_tokens: [u8; NATIVE_STRAIGHT_LONG_MAX_CONTEXT_ENTRIES],
    context_count: u8,
    property_binding_op_indices: [u8; NATIVE_STRAIGHT_LONG_MAX_CONTEXT_ENTRIES],
    property_binding_property_indices: [u8; NATIVE_STRAIGHT_LONG_MAX_CONTEXT_ENTRIES],
    property_binding_slots: [u8; NATIVE_STRAIGHT_LONG_MAX_CONTEXT_ENTRIES],
    property_binding_count: u8,
    call_targets: [*const FunctionCommon; NATIVE_QUICK_LONG_MAX_CALL_TARGETS],
    call_completion_operations: [u8; NATIVE_QUICK_LONG_MAX_CALL_TARGETS],
    call_count: u8,
    trace_guard_operation_indices: [u8; NATIVE_STRAIGHT_LONG_MAX_OPERATIONS],
    trace_guard_condition_slots: [u8; NATIVE_STRAIGHT_LONG_MAX_OPERATIONS],
    trace_guard_expected: [bool; NATIVE_STRAIGHT_LONG_MAX_OPERATIONS],
    trace_guard_count: u8,
    long_output_mask: u64,
    string_output_mask: u64,
    mutable_slots: [u8; NATIVE_QUICK_LONG_SLOT_CAPACITY],
    mutable_slot_count: u8,
}

const QUICK_LONG_BRANCH_CONDITION_LIMIT: usize = 8;

#[derive(Clone, Copy)]
#[cfg(feature = "quick-loops")]
struct QuickLongBranchCondition {
    lhs: u16,
    rhs: QuickLongOperand,
    condition_tmp: Option<u16>,
}

#[derive(Clone, Copy)]
#[cfg(feature = "quick-loops")]
struct QuickLongBranchOnlyKernel {
    header_lhs: u16,
    header_rhs: QuickLongOperand,
    header_condition_tmp: Option<u16>,
    conditions: [QuickLongBranchCondition; QUICK_LONG_BRANCH_CONDITION_LIMIT],
    condition_count: u8,
    post_value: u16,
    post_result: Option<u16>,
    post_resume_ip: usize,
    body_target: QuickLongTarget,
    exit_target: QuickLongTarget,
}

/// Dense no-JIT kernel for accumulating a scalar expression whose leaves are
/// invariant object-property projections. `term_rhs == None` represents one
/// direct property value; otherwise the two property values are added once at
/// region entry and retained in `term_result`.
#[derive(Clone, Copy)]
#[cfg(feature = "quick-loops")]
struct QuickLongInvariantPropertyAccumulateKernel {
    header_lhs: u16,
    header_rhs: QuickLongOperand,
    header_condition_tmp: Option<u16>,
    property_output_mask: u64,
    term_lhs: u16,
    term_rhs: Option<u16>,
    term_result: Option<u16>,
    term_resume_ip: usize,
    accumulator: u16,
    sum_result: u16,
    sum_resume_ip: usize,
    post_value: u16,
    post_result: Option<u16>,
    post_resume_ip: usize,
    body_target: QuickLongTarget,
    exit_target: QuickLongTarget,
}

#[derive(Clone, Copy)]
#[cfg(feature = "quick-loops")]
struct QuickLongAddAssignKernel {
    lhs: u16,
    rhs: u16,
    result: u16,
    destination: u16,
    resume_ip: usize,
}

#[derive(Clone, Copy)]
#[cfg(feature = "quick-loops")]
struct QuickLongAddAddAssignKernel {
    first_lhs: u16,
    first_rhs: u16,
    first_result: u16,
    second_lhs: u16,
    second_rhs: u16,
    second_result: u16,
    destination: u16,
    first_resume_ip: usize,
    second_resume_ip: usize,
}

#[derive(Clone, Copy)]
#[cfg(feature = "quick-loops")]
struct QuickLongConditionalAddAssignKernel {
    condition: QuickLongCondition,
    condition_tmp: Option<u16>,
    lhs: u16,
    rhs: u16,
    result: u16,
    destination: u16,
    add_resume_ip: usize,
}

#[cfg(feature = "quick-loops")]
const QUICK_LONG_ARRAY_PREFIX_LIMIT: usize = 8;

/// One straight scalar operation evaluated before an indexed array fetch.
/// Keeping this target-neutral lets the dense no-JIT kernel and both JIT
/// feature builds share the same guarded key-expression semantics.
#[derive(Clone, Copy)]
#[cfg(feature = "quick-loops")]
struct QuickLongArrayPrefixOp {
    kind: ScalarLongOpKind,
    lhs: QuickLongOperand,
    rhs: QuickLongOperand,
    result: u16,
    destination: Option<u16>,
    resume_ip: usize,
}

#[derive(Clone, Copy)]
#[cfg(feature = "quick-loops")]
enum QuickLongArrayBodyKernel {
    OneAdd {
        add: QuickLongAddAssignKernel,
    },
    TwoAdds {
        first: QuickLongAddAssignKernel,
        second: QuickLongAddAssignKernel,
    },
    AddFusedAddAdd {
        first: QuickLongAddAssignKernel,
        middle: QuickLongAddAddAssignKernel,
        last: QuickLongAddAssignKernel,
    },
    ConditionalAdd {
        first: QuickLongConditionalAddAssignKernel,
        second: QuickLongAddAssignKernel,
    },
}

#[derive(Clone, Copy)]
#[cfg(feature = "quick-loops")]
struct QuickLongArrayLoopKernel {
    header_lhs: u16,
    header_rhs: QuickLongOperand,
    header_condition_tmp: Option<u16>,
    array: u16,
    index: QuickArrayIndex,
    fetch_result: u16,
    fetch_destination: Option<u16>,
    fetch_resume_ip: usize,
    post_value: u16,
    post_result: Option<u16>,
    post_resume_ip: usize,
    body_target: QuickLongTarget,
    exit_target: QuickLongTarget,
}
