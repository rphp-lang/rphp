    #[test]
    fn detects_conditional_body_as_internal_branch() {
        let plan = long_ops_plan(
            "<?php
$n = 100;
$cutoff = 50;
$sum = 0;
for ($i = 0; $i < $n; $i++) {
    if ($i < $cutoff) {
        $sum += $i;
    }
}
",
        );
        assert_eq!(
            plan.ops
                .iter()
                .filter(|op| matches!(op, QuickLongOp::BranchUnlessLt { .. }))
                .count(),
            1
        );
        assert_eq!(plan.entry_op, 0);
        match plan.ops.first() {
            Some(QuickLongOp::BranchUnlessLt {
                false_target,
                next_target,
                ..
            }) => {
                assert!(false_target.exit_ip().is_some());
                assert!(next_target.op_index().is_some());
            }
            op => panic!("expected an entry less-than branch, got {op:?}"),
        }
        assert!(plan.ops.iter().any(|op| matches!(
            op,
            QuickLongOp::ConditionalAddAssign {
                condition: QuickLongCondition::Lt { .. },
                ..
            }
        )));
        assert!(matches!(
            plan.ops.last(),
            Some(QuickLongOp::PostIncLoopLt {
                body_target,
                exit_target,
                ..
            }) if body_target.op_index().is_some() && exit_target.exit_ip().is_some()
        ));
    }

    #[test]
    fn detects_modulo_equality_conditional_body() {
        let plan = long_ops_plan(
            "<?php
$n = 100;
$sum = 0;
for ($i = 0; $i < $n; $i++) {
    if (($i % 2) == 0) {
        $sum += $i;
    }
}
",
        );
        assert!(
            plan.ops
                .iter()
                .any(|op| matches!(op, QuickLongOp::ModConst { divisor: 2, .. }))
        );
        assert!(plan.ops.iter().any(|op| matches!(
            op,
            QuickLongOp::ConditionalAddAssign {
                condition: QuickLongCondition::Eq {
                    rhs: QuickLongOperand::Const(0),
                    ..
                },
                ..
            }
        )));
    }

    #[test]
    fn detects_dynamic_string_array_key_state() {
        let plan = long_ops_plan(
            "<?php
$values = ['left' => 3, 'right' => 5];
$key = 'left';
$sum = 0;
for ($i = 0; $i < 100; $i++) {
    $sum += $values[$key];
    if (($i % 2) == 0) {
        $key = 'right';
    } else {
        $key = 'left';
    }
}
",
        );
        assert!(plan.string_input_mask != 0);
        assert_eq!(plan.string_input_mask, plan.string_output_mask);
        assert_eq!(plan.string_cache_capacity, 2);
        assert_eq!(plan.finite_string_literal_count, 2);
        assert!(!plan.finite_string_literal_overflow);
        assert!(plan.ops.iter().any(|op| matches!(
            op,
            QuickLongOp::FetchArrayLong {
                index: QuickArrayIndex::ValueSlot(_),
                ..
            }
        )));
        assert_eq!(
            plan.ops
                .iter()
                .filter(|op| matches!(op, QuickLongOp::AssignStringLiteral { .. }))
                .count(),
            2
        );
    }

    #[test]
    fn fuses_existing_dynamic_hash_entry_update_without_structural_writes() {
        let plan = long_ops_plan(
            "<?php
$values = ['left' => 3, 'right' => 5];
$key = 'left';
for ($i = 0; $i < 100; $i++) {
    $values[$key] = $values[$key] + $i;
    if (($i % 2) == 0) {
        $key = 'right';
    } else {
        $key = 'left';
    }
}
",
        );
        let fetch = plan
            .ops
            .iter()
            .position(|operation| matches!(operation, QuickLongOp::FetchArrayLong { .. }))
            .expect("dynamic hash fetch");
        let fusion = plan.array_update_fusions[fetch].expect("array update fusion");
        assert_eq!(fusion.kind, ScalarLongOpKind::Add);
        assert!(fusion.next_target.op_index().is_some());
    }

    #[test]
    fn detects_mixed_string_method_and_control_flow_method_in_one_hash_loop() {
        let plan = long_ops_plan(
            "<?php
class Mixer {
    public function score(int $value, string $key): int {
        return $value + strlen($key);
    }
    public function accepted(int $value, int $sequence): int {
        if (($value % 11) == 0 || ($sequence % 17) == 0) { return 1; }
        return 0;
    }
}
function runMixedMethodLoop() {
$mixer = new Mixer();
$values = ['left' => 0, 'right' => 0];
$key = 'left';
$accepted = 0;
$needle = -1;
for ($i = 0; $i < 100; $i++) {
    if (($i % 2) == 0) { $key = 'right'; } else { $key = 'left'; }
    $score = $mixer->score(7, $key);
    $key = 'left';
    $values[$key] = $values[$key] + $score;
    $isAccepted = $mixer->accepted(11, 17);
    $accepted = $accepted + $isAccepted;
    if ($i === $needle) { echo 'never'; }
}
}
",
        );
        assert!(
            plan.ops
                .iter()
                .any(|operation| matches!(operation, QuickLongOp::ObjectLongMethodCall { .. }))
        );
        assert!(
            plan.ops
                .iter()
                .any(|operation| matches!(operation, QuickLongOp::ScalarMethodCall { .. }))
        );
        assert!(
            !plan
                .ops
                .iter()
                .any(|operation| matches!(operation, QuickLongOp::Assign { .. }))
        );
        assert!(plan.ops.iter().any(|operation| matches!(
            operation,
            QuickLongOp::TraceGuard {
                kind: ScalarLongConditionKind::Equal,
                expected: false,
                ..
            }
        )));
        assert!(plan.array_update_fusions.iter().any(Option::is_some));
    }

    #[test]
    fn detects_strict_cold_edge_inside_general_long_ops_loop() {
        let plan = long_ops_plan(
            "<?php
$needle = -1;
$sum = 0;
$count = 0;
for ($i = 0; $i < 100; $i++) {
    $sum = $sum + $i;
    $count = $count + 1;
    if ($i === $needle) {
        echo 'never';
    }
}
",
        );
        let guard_index = plan
            .ops
            .iter()
            .position(|operation| matches!(operation, QuickLongOp::TraceGuard { .. }))
            .expect("strict cold edge should remain inside the general typed loop");
        assert!(matches!(
            plan.ops[guard_index],
            QuickLongOp::TraceGuard {
                kind: ScalarLongConditionKind::Equal,
                expected: false,
                next_target,
                resume_ip,
                ..
            } if next_target.op_index() == Some(guard_index + 1)
                && resume_ip < plan.backedge_ip
        ));
        assert!(matches!(
            plan.ops.last(),
            Some(QuickLongOp::PostIncLoopLt { .. })
        ));
    }

    #[test]
    fn structural_array_push_disables_cached_entry_pointer_fusion() {
        let plan = long_ops_plan(
            "<?php
$values = ['left' => 3];
$key = 'left';
for ($i = 0; $i < 100; $i++) {
    $values[$key] = $values[$key] + 1;
    $values[] = $i;
}
",
        );
        assert!(
            plan.ops
                .iter()
                .any(|operation| matches!(operation, QuickLongOp::StoreArrayLong { .. }))
        );
        assert!(plan.array_update_fusions.iter().all(Option::is_none));
    }

    #[test]
    fn sizes_dynamic_string_cache_from_distinct_loop_literals() {
        let plan = long_ops_plan(
            "<?php
$values = ['left' => 3, 'right' => 5, 'middle' => 7];
$key = 'left';
$sum = 0;
for ($i = 0; $i < 100; $i++) {
    $sum += $values[$key];
    $remainder = $i % 3;
    if ($remainder == 0) {
        $key = 'right';
    } else {
        if ($remainder == 1) {
            $key = 'middle';
        } else {
            $key = 'left';
        }
    }
}
",
        );
        assert_eq!(plan.string_cache_capacity, 3);
    }

    #[test]
    fn detects_dynamic_string_array_key_sources() {
        let plan = long_ops_plan(
            "<?php
$values = ['left' => 3, 'right' => 5];
$left = 'left';
$right = 'right';
$key = $left;
$sum = 0;
for ($i = 0; $i < 100; $i++) {
    $sum += $values[$key];
    if (($i % 2) == 0) {
        $key = $right;
    } else {
        $key = $left;
    }
}
",
        );
        assert_eq!(plan.string_input_mask.count_ones(), 3);
        assert_eq!(plan.string_output_mask.count_ones(), 1);
        assert_eq!(plan.string_cache_capacity, 2);
        assert_eq!(plan.finite_string_literal_count, 2);
        assert!(!plan.finite_string_literal_overflow);
        assert_eq!(
            plan.ops
                .iter()
                .filter(|op| matches!(op, QuickLongOp::AssignStringSlot { .. }))
                .count(),
            2
        );
    }

    #[test]
    fn marks_native_finite_string_tables_that_exceed_the_shared_limit() {
        let plan = long_ops_plan(
            "<?php
$values = ['a' => 1, 'b' => 2, 'c' => 3, 'd' => 4, 'e' => 5];
$key = 'a';
$sum = 0;
for ($i = 0; $i < 100; $i++) {
    $sum += $values[$key];
    $remainder = $i % 5;
    if ($remainder == 0) {
        $key = 'a';
    } else if ($remainder == 1) {
        $key = 'b';
    } else if ($remainder == 2) {
        $key = 'c';
    } else if ($remainder == 3) {
        $key = 'd';
    } else {
        $key = 'e';
    }
}
",
        );
        assert_eq!(plan.finite_string_literal_count, 4);
        assert!(plan.finite_string_literal_overflow);
    }

    #[test]
    fn keeps_dynamic_integer_key_sources_on_long_state() {
        let plan = long_ops_plan(
            "<?php
$values = [100 => 3, 107 => 5];
$left = 100;
$right = 107;
$key = $left;
$sum = 0;
for ($i = 0; $i < 100; $i++) {
    $sum += $values[$key];
    if (($i % 2) == 0) {
        $key = $right;
    } else {
        $key = $left;
    }
}
",
        );
        assert_eq!(plan.string_input_mask, 0);
        assert_eq!(plan.string_output_mask, 0);
        assert_eq!(
            plan.ops
                .iter()
                .filter(|op| matches!(op, QuickLongOp::Assign { .. }))
                .count(),
            2
        );
    }
