use super::{
    CompiledQuickLongConditionalAccumulateLoop, NativeConditionalLongLoopCondition,
    NativeConditionalLongLoopConfig, QuickLongAccumulateJitOutcome,
    conditional_long_remaining_range_is_proven,
};
use crate::vm::quick::QuickLongOperand;
use std::sync::atomic::AtomicBool;

fn less_than_config() -> NativeConditionalLongLoopConfig {
    NativeConditionalLongLoopConfig {
        induction_slot: 0,
        bound: QuickLongOperand::Slot(1),
        condition: NativeConditionalLongLoopCondition::LessThan {
            rhs: QuickLongOperand::Slot(2),
        },
        accumulator_slot: 3,
    }
}

#[test]
fn conservative_conditional_proof_accepts_safe_subsets_and_rejects_extremes() {
    let config = less_than_config();
    let mut slots = [0_i64; 64];
    slots[0] = -100;
    slots[1] = 101;
    slots[3] = 5_000;
    assert!(conditional_long_remaining_range_is_proven(config, &slots));

    slots[0] = 1;
    slots[1] = 3;
    slots[3] = i64::MAX - 1;
    assert!(!conditional_long_remaining_range_is_proven(config, &slots));

    slots[0] = -3;
    slots[1] = -1;
    slots[3] = i64::MIN + 1;
    assert!(!conditional_long_remaining_range_is_proven(config, &slots));
}

#[test]
fn conditional_proof_never_accepts_an_overflowing_selected_prefix() {
    let mut random = 0x7a3c_59d1_84e2_6bf0_u64;
    for _ in 0..20_000 {
        random ^= random << 13;
        random ^= random >> 7;
        random ^= random << 17;
        let induction = random as i64;
        let iterations = (random.rotate_left(19) & 63) + 1;
        let Some(bound) = induction.checked_add(iterations as i64) else {
            continue;
        };
        random = random.wrapping_mul(0x9e37_79b9_7f4a_7c15);
        let accumulator = random as i64;
        let config = NativeConditionalLongLoopConfig {
            induction_slot: 0,
            bound: QuickLongOperand::Const(bound),
            condition: NativeConditionalLongLoopCondition::LessThan {
                rhs: QuickLongOperand::Const(0),
            },
            accumulator_slot: 1,
        };
        let mut slots = [0_i64; 64];
        slots[0] = induction;
        slots[1] = accumulator;
        if !conditional_long_remaining_range_is_proven(config, &slots) {
            continue;
        }

        let mut selected_accumulator = accumulator;
        let mut value = induction;
        for index in 0..iterations {
            // Exercise an arbitrary deterministic subset, independent of the
            // condition encoded in the config just like the proof itself.
            if random.rotate_left(index as u32) & 1 != 0 {
                selected_accumulator = selected_accumulator
                    .checked_add(value)
                    .expect("accepted proof must make every selected prefix safe");
            }
            value = value.checked_add(1).unwrap();
        }
    }
}

#[test]
fn proven_conditional_program_polls_and_resumes_without_rust_chunks() {
    let config = less_than_config();
    let program =
        CompiledQuickLongConditionalAccumulateLoop::compile_range_proven_polling(config, 1_024)
            .expect("range-proven conditional loop should lower");
    let interrupt = AtomicBool::new(true);
    let mut slots = [0_i64; 64];
    slots[0] = 0;
    slots[1] = 10_000;
    slots[2] = 5_000;

    let first = program
        .call_range_proven_polling(&mut slots, 10_000, interrupt.as_ptr() as *const bool, 1_024)
        .unwrap();
    assert_eq!(first.outcome, QuickLongAccumulateJitOutcome::ChunkExhausted);
    assert!(first.addition_executed);
    assert_eq!(slots[0], 1_024);
    assert_eq!(slots[3], 1_023 * 1_024 / 2);

    interrupt.store(false, std::sync::atomic::Ordering::Relaxed);
    let second = program
        .call_range_proven_polling(&mut slots, 10_000, interrupt.as_ptr() as *const bool, 2_048)
        .unwrap();
    assert_eq!(second.outcome, QuickLongAccumulateJitOutcome::Completed);
    assert!(second.addition_executed);
    assert_eq!(slots[0], 10_000);
    assert_eq!(slots[3], 4_999 * 5_000 / 2);
}

#[test]
fn proven_modulo_program_preserves_selected_iterations() {
    let config = NativeConditionalLongLoopConfig {
        induction_slot: 0,
        bound: QuickLongOperand::Const(100),
        condition: NativeConditionalLongLoopCondition::ModuloEqual {
            divisor: 3,
            rhs: QuickLongOperand::Const(0),
        },
        accumulator_slot: 1,
    };
    let program =
        CompiledQuickLongConditionalAccumulateLoop::compile_range_proven_polling(config, 32)
            .expect("range-proven modulo loop should lower");
    let interrupt = AtomicBool::new(false);
    let mut slots = [0_i64; 64];
    let result = program
        .call_range_proven_polling(&mut slots, 100, interrupt.as_ptr() as *const bool, 32)
        .unwrap();
    assert_eq!(result.outcome, QuickLongAccumulateJitOutcome::Completed);
    assert_eq!(slots[0], 100);
    assert_eq!(slots[1], 1_683);
}
