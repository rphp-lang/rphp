use super::{
    CompiledQuickLongAccumulateLoop, NativeLongAccumulateState,
    QuickLongAccumulateJitOutcome, arithmetic_long_chunk_is_range_proven,
};

#[test]
fn proves_common_positive_and_negative_chunks() {
    assert!(arithmetic_long_chunk_is_range_proven(
        0,
        NativeLongAccumulateState {
            induction: 0,
            bound: 100_000,
            accumulator: 0,
        },
        1_024,
    ));
    assert!(arithmetic_long_chunk_is_range_proven(
        0,
        NativeLongAccumulateState {
            induction: -3,
            bound: 4,
            accumulator: i64::MIN + 6,
        },
        1_024,
    ));
    assert!(arithmetic_long_chunk_is_range_proven(
        0,
        NativeLongAccumulateState {
            induction: i64::MAX - 1,
            bound: i64::MAX,
            accumulator: 0,
        },
        1_024,
    ));
}

#[test]
fn rejects_any_intermediate_sum_or_term_overflow() {
    assert!(!arithmetic_long_chunk_is_range_proven(
        0,
        NativeLongAccumulateState {
            induction: -3,
            bound: 4,
            accumulator: i64::MIN + 5,
        },
        1_024,
    ));
    assert!(!arithmetic_long_chunk_is_range_proven(
        0,
        NativeLongAccumulateState {
            induction: 1,
            bound: 4,
            accumulator: i64::MAX - 2,
        },
        1_024,
    ));
    assert!(!arithmetic_long_chunk_is_range_proven(
        2,
        NativeLongAccumulateState {
            induction: i64::MAX - 1,
            bound: i64::MAX,
            accumulator: 0,
        },
        1_024,
    ));
}

#[test]
fn range_proven_program_has_no_checked_overflow_stubs() {
    let checked = CompiledQuickLongAccumulateLoop::compile().unwrap();
    let range_proven =
        CompiledQuickLongAccumulateLoop::compile_range_proven().unwrap();
    assert!(range_proven.code().len() < checked.code().len());

    let mut state = NativeLongAccumulateState {
        induction: -3,
        bound: 4,
        accumulator: 0,
    };
    assert_eq!(
        range_proven.call(&mut state, 1_024).unwrap(),
        QuickLongAccumulateJitOutcome::Completed
    );
    assert_eq!(state.induction, 4);
    assert_eq!(state.accumulator, 0);
}

#[test]
fn closed_form_proof_matches_checked_execution_matrix() {
    fn checked_chunk_is_safe(
        addend: i64,
        mut state: NativeLongAccumulateState,
        iteration_budget: u64,
    ) -> bool {
        if iteration_budget == 0 {
            return false;
        }
        for _ in 0..iteration_budget {
            if state.induction >= state.bound {
                return true;
            }
            let Some(term) = state.induction.checked_add(addend) else {
                return false;
            };
            let Some(accumulator) = state.accumulator.checked_add(term) else {
                return false;
            };
            let Some(induction) = state.induction.checked_add(1) else {
                return false;
            };
            state.accumulator = accumulator;
            state.induction = induction;
        }
        true
    }

    let edge_values = [
        i64::MIN,
        i64::MIN + 1,
        -1_024,
        -3,
        -1,
        0,
        1,
        3,
        1_024,
        i64::MAX - 1,
        i64::MAX,
    ];
    for induction in edge_values {
        for bound in edge_values {
            for accumulator in edge_values {
                for addend in edge_values {
                    for budget in [0, 1, 2, 3, 32, 64] {
                        let state = NativeLongAccumulateState {
                            induction,
                            bound,
                            accumulator,
                        };
                        assert_eq!(
                            arithmetic_long_chunk_is_range_proven(
                                addend, state, budget,
                            ),
                            checked_chunk_is_safe(addend, state, budget),
                            "mismatch for addend={addend}, state={state:?}, budget={budget}"
                        );
                    }
                }
            }
        }
    }

    let mut random = 0xd1b5_4a32_d192_ed03_u64;
    for _ in 0..100_000 {
        let mut next = || {
            random = random
                .wrapping_mul(6_364_136_223_846_793_005)
                .wrapping_add(1_442_695_040_888_963_407);
            random as i64
        };
        let state = NativeLongAccumulateState {
            induction: next(),
            bound: next(),
            accumulator: next(),
        };
        let addend = next();
        let budget = (next() as u64) & 63;
        assert_eq!(
            arithmetic_long_chunk_is_range_proven(addend, state, budget),
            checked_chunk_is_safe(addend, state, budget),
            "random mismatch for addend={addend}, state={state:?}, budget={budget}"
        );
    }
}
