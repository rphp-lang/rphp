#[test]
fn invariant_operands_are_loaded_once_per_native_entry() {
    let mut operations =
        [NativeStraightLongOperation::Unused; super::super::NATIVE_STRAIGHT_LONG_MAX_OPERATIONS];
    operations[0] = NativeStraightLongOperation::BranchUnless {
        kind: ScalarLongConditionKind::LessThan,
        lhs: NativeStraightLongConditionOperand::Source(QuickLongOperand::Slot(0)),
        rhs: NativeStraightLongConditionOperand::Source(QuickLongOperand::Slot(3)),
        false_target: 3,
    };
    operations[1] = NativeStraightLongOperation::BinaryAssign {
        kind: ScalarLongOpKind::Add,
        lhs: QuickLongOperand::Slot(0),
        rhs: QuickLongOperand::Slot(3),
        result: 6,
        destination: 1,
    };
    operations[2] = NativeStraightLongOperation::Jump { target: 4 };
    operations[3] = NativeStraightLongOperation::BinaryAssign {
        kind: ScalarLongOpKind::Subtract,
        lhs: QuickLongOperand::Slot(0),
        rhs: QuickLongOperand::Const(1),
        result: 7,
        destination: 1,
    };
    operations[4] = NativeStraightLongOperation::BinaryAssign {
        kind: ScalarLongOpKind::Add,
        lhs: QuickLongOperand::Slot(1),
        rhs: QuickLongOperand::Slot(4),
        result: 8,
        destination: 2,
    };
    let program = CompiledX86StraightLongLoop::compile(NativeStraightLongLoopConfig {
        induction_slot: 0,
        bound: QuickLongOperand::Const(100),
        operations,
        operation_count: 5,
        post_result: None,
    })
    .unwrap();

    let slot_3_load = [0x4c, 0x8b, 0xaf, 0x18, 0x00, 0x00, 0x00];
    let slot_4_load = [0x4c, 0x8b, 0xb7, 0x20, 0x00, 0x00, 0x00];
    assert_eq!(
        program
            .code()
            .windows(slot_3_load.len())
            .filter(|window| *window == slot_3_load)
            .count(),
        5,
        "each of the five ABI entries should load invariant slot 3 once"
    );
    assert_eq!(
        program
            .code()
            .windows(slot_4_load.len())
            .filter(|window| *window == slot_4_load)
            .count(),
        5,
        "each of the five ABI entries should load invariant slot 4 once"
    );
    let dedicated_bound_load = [0x48, 0xb9, 0x64, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00];
    assert_eq!(
        program
            .code()
            .windows(dedicated_bound_load.len())
            .filter(|window| *window == dedicated_bound_load)
            .count(),
        5,
        "constant bounds should stay in RCX unless a fourth resident value uses it"
    );

    let mut slots = [0i64; 64];
    slots[3] = 50;
    slots[4] = 7;
    let outcome = program.call_chunk(&mut slots, 128).unwrap();
    assert_eq!(outcome.outcome, NativeStraightLongLoopOutcome::Completed);
    assert_eq!(
        (slots[0], slots[1], slots[2], slots[3], slots[4]),
        (100, 98, 105, 50, 7)
    );
}

#[test]
fn finite_string_hash_context_survives_signed_division_abi_registers() {
    let mut operations =
        [NativeStraightLongOperation::Unused; super::super::NATIVE_STRAIGHT_LONG_MAX_OPERATIONS];
    operations[0] = NativeStraightLongOperation::StringToken {
        token: 1,
        result: 2,
    };
    operations[1] = NativeStraightLongOperation::StringLength {
        source: 2,
        lengths: [4, 5, 0, 0],
        token_count: 2,
        result: 3,
    };
    operations[2] = NativeStraightLongOperation::HashLoad {
        key: 2,
        entry_base: 0,
        token_count: 2,
        result: 4,
        destination: None,
    };
    operations[3] = NativeStraightLongOperation::Binary {
        kind: ScalarLongOpKind::IntDivide,
        lhs: QuickLongOperand::Slot(4),
        rhs: QuickLongOperand::Slot(3),
        result: 5,
    };
    operations[4] = NativeStraightLongOperation::HashStore {
        key: 2,
        entry_base: 0,
        token_count: 2,
        source: QuickLongOperand::Slot(5),
    };
    let program = CompiledX86StraightLongLoop::compile(NativeStraightLongLoopConfig {
        induction_slot: 0,
        bound: QuickLongOperand::Slot(1),
        operations,
        operation_count: 5,
        post_result: None,
    })
    .expect("finite String and contextual hash operations should lower on x86");
    assert!(
        !program
            .code()
            .windows(2)
            .any(|window| window == [0x41, 0x55]),
        "mixed context entries must not pay the scalar invariant R13 prologue"
    );

    let mut left = 7i64;
    let mut right = 20i64;
    let mut entries =
        [std::ptr::null_mut(); super::super::NATIVE_STRAIGHT_LONG_MAX_CONTEXT_ENTRIES];
    entries[0] = &mut left;
    entries[1] = &mut right;
    let mut slots = [0i64; 64];
    slots[1] = 1;
    let outcome = program
        .call_chunk_with_context(&mut slots, 8, &entries)
        .unwrap();

    assert_eq!(outcome.outcome, NativeStraightLongLoopOutcome::Completed);
    assert_eq!(slots[0], 1);
    assert_eq!(slots[2], 1);
    assert_eq!(slots[3], 5);
    assert_eq!(slots[4], 20);
    assert_eq!(slots[5], 4);
    assert_eq!(left, 7);
    assert_eq!(right, 4);
}

#[test]
fn invalid_finite_string_token_side_exits_before_hash_mutation() {
    let mut operations =
        [NativeStraightLongOperation::Unused; super::super::NATIVE_STRAIGHT_LONG_MAX_OPERATIONS];
    operations[0] = NativeStraightLongOperation::StringLength {
        source: 2,
        lengths: [4, 5, 0, 0],
        token_count: 2,
        result: 3,
    };
    operations[1] = NativeStraightLongOperation::HashStore {
        key: 2,
        entry_base: 0,
        token_count: 2,
        source: QuickLongOperand::Const(99),
    };
    let program = CompiledX86StraightLongLoop::compile(NativeStraightLongLoopConfig {
        induction_slot: 0,
        bound: QuickLongOperand::Slot(1),
        operations,
        operation_count: 2,
        post_result: None,
    })
    .expect("guarded finite String operations should lower on x86");

    let mut left = 7i64;
    let mut right = 20i64;
    let mut entries =
        [std::ptr::null_mut(); super::super::NATIVE_STRAIGHT_LONG_MAX_CONTEXT_ENTRIES];
    entries[0] = &mut left;
    entries[1] = &mut right;
    let mut slots = [0i64; 64];
    slots[1] = 1;
    slots[2] = 3;
    slots[3] = -1;
    let outcome = program
        .call_chunk_with_context(&mut slots, 8, &entries)
        .unwrap();

    assert_eq!(
        outcome.outcome,
        NativeStraightLongLoopOutcome::OperationSideExit
    );
    assert_eq!(outcome.failed_operation, Some(0));
    assert_eq!(slots[0], 0);
    assert_eq!(slots[3], -1);
    assert_eq!(left, 7);
    assert_eq!(right, 20);
}
use crate::jit::NATIVE_STRAIGHT_LONG_MAX_OPERATIONS;
use crate::vm::function::{ScalarLongProgram, ScalarLongSelect};

fn additive_recurrence(bound: i64, reversed: bool) -> NativeStraightLongLoopConfig {
    let mut operations = [NativeStraightLongOperation::Unused; NATIVE_STRAIGHT_LONG_MAX_OPERATIONS];
    let (lhs, rhs) = if reversed {
        (QuickLongOperand::Slot(0), QuickLongOperand::Slot(1))
    } else {
        (QuickLongOperand::Slot(1), QuickLongOperand::Slot(0))
    };
    operations[0] = NativeStraightLongOperation::BinaryAssign {
        kind: ScalarLongOpKind::Add,
        lhs,
        rhs,
        result: 2,
        destination: 1,
    };
    NativeStraightLongLoopConfig {
        induction_slot: 0,
        bound: QuickLongOperand::Const(bound),
        operations,
        operation_count: 1,
        post_result: None,
    }
}

fn composed_add_recurrence(bound: i64) -> NativeStraightLongLoopConfig {
    let mut operations = [NativeStraightLongOperation::Unused; NATIVE_STRAIGHT_LONG_MAX_OPERATIONS];
    operations[0] = NativeStraightLongOperation::Binary {
        kind: ScalarLongOpKind::Add,
        lhs: QuickLongOperand::Slot(0),
        rhs: QuickLongOperand::Const(1),
        result: 4,
    };
    operations[1] = NativeStraightLongOperation::BinaryAssign {
        kind: ScalarLongOpKind::Add,
        lhs: QuickLongOperand::Slot(1),
        rhs: QuickLongOperand::Slot(4),
        result: 2,
        destination: 1,
    };
    NativeStraightLongLoopConfig {
        induction_slot: 0,
        bound: QuickLongOperand::Const(bound),
        operations,
        operation_count: 2,
        post_result: Some(5),
    }
}

fn structured_recurrence(bound: i64) -> NativeStraightLongLoopConfig {
    let mut operations = [NativeStraightLongOperation::Unused; NATIVE_STRAIGHT_LONG_MAX_OPERATIONS];
    operations[0] = NativeStraightLongOperation::BranchUnless {
        kind: ScalarLongConditionKind::LessThan,
        lhs: NativeStraightLongConditionOperand::Source(QuickLongOperand::Slot(0)),
        rhs: NativeStraightLongConditionOperand::Source(QuickLongOperand::Const(2)),
        false_target: 3,
    };
    operations[1] = NativeStraightLongOperation::BinaryAssign {
        kind: ScalarLongOpKind::Add,
        lhs: QuickLongOperand::Slot(1),
        rhs: QuickLongOperand::Const(10),
        result: 2,
        destination: 1,
    };
    operations[2] = NativeStraightLongOperation::Jump { target: 4 };
    operations[3] = NativeStraightLongOperation::BinaryAssign {
        kind: ScalarLongOpKind::Add,
        lhs: QuickLongOperand::Slot(1),
        rhs: QuickLongOperand::Const(100),
        result: 2,
        destination: 1,
    };
    operations[4] = NativeStraightLongOperation::BinaryAssign {
        kind: ScalarLongOpKind::Add,
        lhs: QuickLongOperand::Slot(1),
        rhs: QuickLongOperand::Const(1),
        result: 2,
        destination: 1,
    };
    NativeStraightLongLoopConfig {
        induction_slot: 0,
        bound: QuickLongOperand::Const(bound),
        operations,
        operation_count: 5,
        post_result: None,
    }
}

fn structured_affine_expression(bound: i64) -> NativeStraightLongLoopConfig {
    let mut operations = [NativeStraightLongOperation::Unused; NATIVE_STRAIGHT_LONG_MAX_OPERATIONS];
    operations[0] = NativeStraightLongOperation::BranchUnless {
        kind: ScalarLongConditionKind::LessThan,
        lhs: NativeStraightLongConditionOperand::Source(QuickLongOperand::Slot(0)),
        rhs: NativeStraightLongConditionOperand::Source(QuickLongOperand::Const(50)),
        false_target: 4,
    };
    operations[1] = NativeStraightLongOperation::Binary {
        kind: ScalarLongOpKind::Multiply,
        lhs: QuickLongOperand::Slot(0),
        rhs: QuickLongOperand::Const(3),
        result: 6,
    };
    operations[2] = NativeStraightLongOperation::BinaryAssign {
        kind: ScalarLongOpKind::Add,
        lhs: QuickLongOperand::Slot(6),
        rhs: QuickLongOperand::Const(1),
        result: 7,
        destination: 1,
    };
    operations[3] = NativeStraightLongOperation::Jump { target: 6 };
    operations[4] = NativeStraightLongOperation::Binary {
        kind: ScalarLongOpKind::Multiply,
        lhs: QuickLongOperand::Slot(0),
        rhs: QuickLongOperand::Const(5),
        result: 8,
    };
    operations[5] = NativeStraightLongOperation::BinaryAssign {
        kind: ScalarLongOpKind::Subtract,
        lhs: QuickLongOperand::Slot(8),
        rhs: QuickLongOperand::Const(2),
        result: 9,
        destination: 1,
    };
    operations[6] = NativeStraightLongOperation::Binary {
        kind: ScalarLongOpKind::Multiply,
        lhs: QuickLongOperand::Slot(1),
        rhs: QuickLongOperand::Const(3),
        result: 10,
    };
    operations[7] = NativeStraightLongOperation::BinaryAssign {
        kind: ScalarLongOpKind::Add,
        lhs: QuickLongOperand::Slot(10),
        rhs: QuickLongOperand::Const(11),
        result: 11,
        destination: 2,
    };
    NativeStraightLongLoopConfig {
        induction_slot: 0,
        bound: QuickLongOperand::Const(bound),
        operations,
        operation_count: 8,
        post_result: None,
    }
}

fn scheduled_increment_between_affine_pair(bound: i64) -> NativeStraightLongLoopConfig {
    let mut operations = [NativeStraightLongOperation::Unused; NATIVE_STRAIGHT_LONG_MAX_OPERATIONS];
    operations[0] = NativeStraightLongOperation::BranchUnless {
        kind: ScalarLongConditionKind::LessThan,
        lhs: NativeStraightLongConditionOperand::Source(QuickLongOperand::Slot(0)),
        rhs: NativeStraightLongConditionOperand::Source(QuickLongOperand::Const(0)),
        false_target: 2,
    };
    operations[1] = NativeStraightLongOperation::Jump { target: 2 };
    operations[2] = NativeStraightLongOperation::Binary {
        kind: ScalarLongOpKind::Multiply,
        lhs: QuickLongOperand::Slot(0),
        rhs: QuickLongOperand::Const(3),
        result: 4,
    };
    operations[3] = NativeStraightLongOperation::BinaryAssign {
        kind: ScalarLongOpKind::Add,
        lhs: QuickLongOperand::Slot(4),
        rhs: QuickLongOperand::Const(11),
        result: 5,
        destination: 1,
    };
    NativeStraightLongLoopConfig {
        induction_slot: 0,
        bound: QuickLongOperand::Const(bound),
        operations,
        operation_count: 4,
        post_result: None,
    }
}

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

fn conditional_scalar_plan(
    public_args: u8,
    operations: Vec<ScalarLongOp>,
    select: ScalarLongSelect,
) -> ScalarLongFunctionPlan {
    ScalarLongFunctionPlan::new(
        public_args,
        ScalarLongProgram {
            operations: operations.into_boxed_slice(),
            outputs: [select.when_true],
            output_count: 1,
        },
        Some(select),
    )
}
