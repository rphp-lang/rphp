use super::{
    CompiledQuickLongStraightLoop, NativeStraightLongLoopConfig, NativeStraightLongLoopOutcome,
    NativeStraightLongOperation,
};
use crate::jit::straight::NATIVE_STRAIGHT_LONG_MAX_OPERATIONS;
use crate::vm::function::ScalarLongOpKind;
use crate::vm::quick::QuickLongOperand;
use std::sync::atomic::AtomicBool;

fn independent_published_results_config(bound: i64) -> NativeStraightLongLoopConfig {
    let mut operations = [NativeStraightLongOperation::Unused; NATIVE_STRAIGHT_LONG_MAX_OPERATIONS];
    operations[0] = NativeStraightLongOperation::Binary {
        kind: ScalarLongOpKind::Add,
        lhs: QuickLongOperand::Slot(0),
        rhs: QuickLongOperand::Const(7),
        result: 2,
    };
    operations[1] = NativeStraightLongOperation::Binary {
        kind: ScalarLongOpKind::Add,
        lhs: QuickLongOperand::Slot(0),
        rhs: QuickLongOperand::Const(11),
        result: 3,
    };
    NativeStraightLongLoopConfig {
        induction_slot: 0,
        bound: QuickLongOperand::Const(bound),
        operations,
        operation_count: 2,
        post_result: None,
    }
}

fn single_composed_recurrence_config(bound: i64) -> NativeStraightLongLoopConfig {
    let mut operations = [NativeStraightLongOperation::Unused; NATIVE_STRAIGHT_LONG_MAX_OPERATIONS];
    operations[0] = NativeStraightLongOperation::Binary {
        kind: ScalarLongOpKind::Multiply,
        lhs: QuickLongOperand::Slot(0),
        rhs: QuickLongOperand::Const(3),
        result: 2,
    };
    operations[1] = NativeStraightLongOperation::BinaryAssign {
        kind: ScalarLongOpKind::Add,
        lhs: QuickLongOperand::Slot(3),
        rhs: QuickLongOperand::Slot(2),
        result: 4,
        destination: 3,
    };
    NativeStraightLongLoopConfig {
        induction_slot: 0,
        bound: QuickLongOperand::Const(bound),
        operations,
        operation_count: 2,
        post_result: None,
    }
}

#[test]
fn linear_publication_generates_earlier_result_in_its_fixed_register() {
    let program =
        CompiledQuickLongStraightLoop::compile_range_proven_polling_with_publication_and_carried(
            independent_published_results_config(5),
            1_024,
            (1u64 << 2) | (1u64 << 3),
            0,
        )
        .unwrap();
    let interrupt = AtomicBool::new(false);
    let mut slots = [0_i64; 64];

    let completed = program
        .call_range_proven_polling(&mut slots, 5, interrupt.as_ptr() as *const bool, 5)
        .unwrap();
    assert_eq!(completed.outcome, NativeStraightLongLoopOutcome::Completed);
    assert_eq!((slots[0], slots[2], slots[3]), (5, 11, 15));

    let words = program
        .code()
        .chunks_exact(4)
        .map(|bytes| u32::from_le_bytes(bytes.try_into().unwrap()))
        .collect::<Vec<_>>();
    assert!(words.contains(&0x9100_1c64)); // ADD X4, X3, #7
    assert!(!words.contains(&0xaa08_03e4)); // MOV X4, X8
}

#[test]
fn single_composed_recurrence_retains_the_profitable_rename_move() {
    let program =
        CompiledQuickLongStraightLoop::compile_range_proven_polling_with_publication_and_carried(
            single_composed_recurrence_config(5_000),
            1_024,
            1u64 << 3,
            1u64 << 3,
        )
        .unwrap();
    let interrupt = AtomicBool::new(false);
    let mut slots = [0_i64; 64];
    slots[3] = 10;

    let completed = program
        .call_range_proven_polling(&mut slots, 5_000, interrupt.as_ptr() as *const bool, 5_000)
        .unwrap();
    assert_eq!(completed.outcome, NativeStraightLongLoopOutcome::Completed);
    assert_eq!((slots[0], slots[3]), (5_000, 37_492_510));

    let words = program
        .code()
        .chunks_exact(4)
        .map(|bytes| u32::from_le_bytes(bytes.try_into().unwrap()))
        .collect::<Vec<_>>();
    assert!(words.contains(&0x9b07_7c68)); // MUL X8, X3, X7
    assert!(words.contains(&0x8b08_0088)); // ADD X8, X4, X8
    assert!(words.contains(&0xaa08_03e4)); // MOV X4, X8
}
