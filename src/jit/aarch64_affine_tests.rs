use super::{
    Arm64Assembler, Arm64Register, CompiledQuickLongStraightLoop, NativeStraightLongLoopConfig,
    NativeStraightLongLoopOutcome, NativeStraightLongOperation,
};
use crate::jit::straight::NATIVE_STRAIGHT_LONG_MAX_OPERATIONS;
use crate::vm::function::ScalarLongOpKind;
use crate::vm::quick::QuickLongOperand;
use std::sync::atomic::{AtomicBool, Ordering};

fn variable_multiply_add_config(bound: i64) -> NativeStraightLongLoopConfig {
    let mut operations = [NativeStraightLongOperation::Unused; NATIVE_STRAIGHT_LONG_MAX_OPERATIONS];
    operations[0] = NativeStraightLongOperation::Binary {
        kind: ScalarLongOpKind::Multiply,
        lhs: QuickLongOperand::Slot(0),
        rhs: QuickLongOperand::Slot(1),
        result: 4,
    };
    operations[1] = NativeStraightLongOperation::BinaryAssign {
        kind: ScalarLongOpKind::Add,
        lhs: QuickLongOperand::Slot(4),
        rhs: QuickLongOperand::Slot(2),
        result: 5,
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

fn variable_reverse_multiply_subtract_config(bound: i64) -> NativeStraightLongLoopConfig {
    let mut config = variable_multiply_add_config(bound);
    config.operations[1] = NativeStraightLongOperation::BinaryAssign {
        kind: ScalarLongOpKind::Subtract,
        lhs: QuickLongOperand::Slot(2),
        rhs: QuickLongOperand::Slot(4),
        result: 5,
        destination: 3,
    };
    config
}

fn carried_multiply_add_config(bound: i64) -> NativeStraightLongLoopConfig {
    let mut config = variable_multiply_add_config(bound);
    config.operations[1] = NativeStraightLongOperation::BinaryAssign {
        kind: ScalarLongOpKind::Add,
        lhs: QuickLongOperand::Slot(4),
        rhs: QuickLongOperand::Slot(3),
        result: 5,
        destination: 3,
    };
    config
}

fn scheduled_increment_between_pair_config(bound: i64) -> NativeStraightLongLoopConfig {
    let mut operations = [NativeStraightLongOperation::Unused; NATIVE_STRAIGHT_LONG_MAX_OPERATIONS];
    operations[0] = NativeStraightLongOperation::BranchUnless {
        kind: crate::vm::function::ScalarLongConditionKind::LessThan,
        lhs: crate::jit::straight::NativeStraightLongConditionOperand::Source(
            QuickLongOperand::Slot(0),
        ),
        rhs: crate::jit::straight::NativeStraightLongConditionOperand::Source(
            QuickLongOperand::Const(0),
        ),
        false_target: 2,
    };
    operations[1] = NativeStraightLongOperation::Jump { target: 2 };
    operations[2] = NativeStraightLongOperation::Binary {
        kind: ScalarLongOpKind::Multiply,
        lhs: QuickLongOperand::Slot(0),
        rhs: QuickLongOperand::Slot(1),
        result: 4,
    };
    operations[3] = NativeStraightLongOperation::BinaryAssign {
        kind: ScalarLongOpKind::Add,
        lhs: QuickLongOperand::Slot(4),
        rhs: QuickLongOperand::Slot(2),
        result: 5,
        destination: 3,
    };
    NativeStraightLongLoopConfig {
        induction_slot: 0,
        bound: QuickLongOperand::Const(bound),
        operations,
        operation_count: 4,
        post_result: None,
    }
}

fn is_real_multiply_add(word: u32) -> bool {
    word & 0xffe0_8000 == 0x9b00_0000 && (word >> 10) & 31 != 31
}

fn is_multiply_subtract(word: u32) -> bool {
    word & 0xffe0_8000 == 0x9b00_8000
}

#[test]
fn encoder_produces_exact_madd_and_msub_words() {
    let mut assembler = Arm64Assembler::new();
    assembler.multiply_add(
        Arm64Register::from_code(8),
        Arm64Register::from_code(3),
        Arm64Register::from_code(10),
        Arm64Register::from_code(9),
    );
    assembler.multiply_subtract(
        Arm64Register::from_code(4),
        Arm64Register::from_code(3),
        Arm64Register::from_code(10),
        Arm64Register::from_code(9),
    );
    assert_eq!(
        assembler.finish(),
        [0x68, 0x24, 0x0a, 0x9b, 0x64, 0xa4, 0x0a, 0x9b]
    );
}

#[test]
fn range_proven_polling_fuses_variable_multiply_add() {
    let program =
        CompiledQuickLongStraightLoop::compile_range_proven_polling_with_publication_and_carried(
            variable_multiply_add_config(5_000),
            1_024,
            (1u64 << 3) | (1u64 << 5),
            0,
        )
        .unwrap();
    let interrupt = AtomicBool::new(true);
    let mut slots = [0_i64; 64];
    slots[1] = 73;
    slots[2] = 19;

    let interrupted = program
        .call_range_proven_polling(&mut slots, 5_000, interrupt.as_ptr() as *const bool, 1_024)
        .unwrap();
    assert_eq!(
        interrupted.outcome,
        NativeStraightLongLoopOutcome::ChunkExhausted
    );
    assert_eq!((slots[0], slots[3], slots[5]), (1_024, 74_698, 74_698));

    interrupt.store(false, Ordering::Relaxed);
    let completed = program
        .call_range_proven_polling(&mut slots, 5_000, interrupt.as_ptr() as *const bool, 2_048)
        .unwrap();
    assert_eq!(completed.outcome, NativeStraightLongLoopOutcome::Completed);
    assert_eq!((slots[0], slots[3], slots[5]), (5_000, 364_946, 364_946));

    let madd_words = program
        .code()
        .chunks_exact(4)
        .map(|bytes| u32::from_le_bytes(bytes.try_into().unwrap()))
        .filter(|word| is_real_multiply_add(*word))
        .collect::<Vec<_>>();
    assert_eq!(madd_words, [0x9b0a_2468]);
}

#[test]
fn carried_multiply_add_keeps_a_rename_move_after_the_fused_result() {
    let program =
        CompiledQuickLongStraightLoop::compile_range_proven_polling_with_publication_and_carried(
            carried_multiply_add_config(5_000),
            1_024,
            1u64 << 3,
            1u64 << 3,
        )
        .unwrap();
    let interrupt = AtomicBool::new(false);
    let mut slots = [0_i64; 64];
    slots[1] = 73;
    slots[3] = 10;

    let completed = program
        .call_range_proven_polling(&mut slots, 5_000, interrupt.as_ptr() as *const bool, 5_000)
        .unwrap();
    assert_eq!(completed.outcome, NativeStraightLongLoopOutcome::Completed);
    assert_eq!((slots[0], slots[3]), (5_000, 912_317_510));

    let words = program
        .code()
        .chunks_exact(4)
        .map(|bytes| u32::from_le_bytes(bytes.try_into().unwrap()))
        .collect::<Vec<_>>();
    assert!(words.contains(&0x9b0a_1068)); // MADD X8, X3, X10, X4
    assert!(words.contains(&0xaa08_03e4)); // MOV X4, X8
}

#[test]
fn range_proven_polling_fuses_variable_reverse_multiply_subtract() {
    let program =
        CompiledQuickLongStraightLoop::compile_range_proven_polling_with_publication_and_carried(
            variable_reverse_multiply_subtract_config(5_000),
            1_024,
            (1u64 << 3) | (1u64 << 5),
            0,
        )
        .unwrap();
    let interrupt = AtomicBool::new(false);
    let mut slots = [0_i64; 64];
    slots[1] = 73;
    slots[2] = 19;

    let completed = program
        .call_range_proven_polling(&mut slots, 5_000, interrupt.as_ptr() as *const bool, 1_024)
        .unwrap();
    assert_eq!(completed.outcome, NativeStraightLongLoopOutcome::Completed);
    assert_eq!((slots[0], slots[3], slots[5]), (5_000, -364_908, -364_908));
    let msub_words = program
        .code()
        .chunks_exact(4)
        .map(|bytes| u32::from_le_bytes(bytes.try_into().unwrap()))
        .filter(|word| is_multiply_subtract(*word))
        .collect::<Vec<_>>();
    assert_eq!(msub_words, [0x9b0a_a468]);
}

#[test]
fn range_proven_polling_preserves_published_multiply_intermediate() {
    let program =
        CompiledQuickLongStraightLoop::compile_range_proven_polling_with_publication_and_carried(
            variable_multiply_add_config(5_000),
            1_024,
            (1u64 << 3) | (1u64 << 4) | (1u64 << 5),
            0,
        )
        .unwrap();
    let interrupt = AtomicBool::new(false);
    let mut slots = [0_i64; 64];
    slots[1] = 73;
    slots[2] = 19;

    let completed = program
        .call_range_proven_polling(&mut slots, 5_000, interrupt.as_ptr() as *const bool, 1_024)
        .unwrap();
    assert_eq!(completed.outcome, NativeStraightLongLoopOutcome::Completed);
    assert_eq!(
        (slots[0], slots[3], slots[4], slots[5]),
        (5_000, 364_946, 364_927, 364_946)
    );
    assert!(
        program
            .code()
            .chunks_exact(4)
            .map(|bytes| u32::from_le_bytes(bytes.try_into().unwrap()))
            .all(|word| !is_real_multiply_add(word))
    );
}

#[test]
fn range_proven_polling_does_not_fuse_across_scheduled_induction_increment() {
    let program =
        CompiledQuickLongStraightLoop::compile_range_proven_polling_with_publication_and_carried(
            scheduled_increment_between_pair_config(5_000),
            1_024,
            (1u64 << 3) | (1u64 << 5),
            0,
        )
        .unwrap();
    let interrupt = AtomicBool::new(false);
    let mut slots = [0_i64; 64];
    slots[1] = 73;
    slots[2] = 19;

    let completed = program
        .call_range_proven_polling(&mut slots, 5_000, interrupt.as_ptr() as *const bool, 1_024)
        .unwrap();
    assert_eq!(completed.outcome, NativeStraightLongLoopOutcome::Completed);
    assert_eq!((slots[0], slots[3], slots[5]), (5_000, 364_946, 364_946));
    assert!(
        program
            .code()
            .chunks_exact(4)
            .map(|bytes| u32::from_le_bytes(bytes.try_into().unwrap()))
            .all(|word| !is_real_multiply_add(word))
    );
}
