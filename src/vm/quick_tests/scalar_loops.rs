    #[test]
    fn detects_induction_plus_constant_with_cv_bound() {
        let plan = quick_plan(
            "<?php
$n = 100;
$sum = 0;
for ($i = 0; $i < $n; $i++) {
    $sum += $i + 1;
}
",
        );
        assert_eq!(plan.induction_cv, 2);
        assert_eq!(plan.accumulator_cv, 1);
        assert!(matches!(plan.bound, QuickLongBound::Cv(0)));
        assert!(matches!(
            plan.term,
            QuickLongTerm::InductionPlusConst { addend: 1, .. }
        ));
        assert_eq!(plan.exit_ip, 10);
    }

    #[test]
    fn detects_induction_plus_invariant_cv_in_either_order() {
        for expression in ["$i + $offset", "$offset + $i"] {
            let plan = quick_plan(&format!(
                "<?php
$offset = 7;
$sum = 0;
for ($i = 0; $i < 100; $i++) {{
    $sum += {expression};
}}
"
            ));
            assert_eq!(plan.induction_cv, 2);
            assert_eq!(plan.accumulator_cv, 1);
            assert!(matches!(
                plan.term,
                QuickLongTerm::InductionPlusCv { addend_cv: 0, .. }
            ));
        }
    }

    #[test]
    fn detects_direct_scalar_function_call_accumulation() {
        let plan = quick_plan(
            "<?php
function affine($value, $scale, $bias) {
    return $value * $scale + $bias;
}
$scale = 2;
$sum = 0;
for ($i = 0; $i < 100; $i++) {
    $sum += affine($i, $scale, 1);
}
",
        );
        assert_eq!(plan.induction_cv, 2);
        assert_eq!(plan.accumulator_cv, 1);
        assert!(matches!(
            plan.term,
            QuickLongTerm::ScalarFunctionCall {
                argument_count: 3,
                long_input_mask,
                guard,
                do_fcall_ip,
                ..
            } if long_input_mask == 0
                && matches!(guard, ScalarLongCallGuard::FunctionCache { .. })
                && do_fcall_ip == guard.cache_ip() + 4
        ));
    }

    #[test]
    fn detects_scalar_expression_in_function_call_argument() {
        let plan = quick_plan(
            "<?php
function combine($left, $right) {
    return $left + $right;
}
$sum = 0;
for ($i = 0; $i < 100; $i++) {
    $sum += combine($i, $i + 1);
}
",
        );
        assert!(matches!(
            plan.term,
            QuickLongTerm::ScalarFunctionCall {
                argument_count: 2,
                long_input_mask,
                guard,
                do_fcall_ip,
                ..
            } if long_input_mask == 1u64 << 1
                && matches!(guard, ScalarLongCallGuard::FunctionCache { .. })
                && do_fcall_ip == guard.cache_ip() + 4
        ));
    }

    #[test]
    fn compiles_scalar_arguments_into_typed_plan() {
        let plan = quick_plan(
            "<?php
function combine($left, $right) {
    return $left + $right;
}
$sum = 0;
for ($i = 0; $i < 100; $i++) {
    $sum += combine($i, $i + 1);
}
",
        );
        let QuickLongTerm::ScalarFunctionCall { argument_plan, .. } = plan.term else {
            panic!("expected scalar function call term");
        };
        assert_eq!(argument_plan.operations.len(), 1);
        assert!(matches!(
            argument_plan.operations[0],
            ScalarLongOp {
                kind: ScalarLongOpKind::Add,
                lhs: ScalarLongSource::Input(1),
                rhs: ScalarLongSource::Constant(1),
            }
        ));
        assert_eq!(argument_plan.outputs[0], ScalarLongSource::Input(1));
        assert_eq!(argument_plan.outputs[1], ScalarLongSource::Temporary(0));
    }

    #[test]
    fn composes_argument_and_leaf_program_temporary_indices() {
        let mut argument_outputs = [ScalarLongSource::Constant(0); 8];
        argument_outputs[0] = ScalarLongSource::Input(1);
        argument_outputs[1] = ScalarLongSource::Temporary(0);
        let arguments = ScalarLongProgram {
            operations: vec![ScalarLongOp {
                kind: ScalarLongOpKind::Add,
                lhs: ScalarLongSource::Input(1),
                rhs: ScalarLongSource::Constant(1),
            }]
            .into_boxed_slice(),
            outputs: argument_outputs,
            output_count: 2,
        };
        let body = ScalarLongFunctionPlan::new(
            2,
            ScalarLongProgram {
                operations: vec![
                    ScalarLongOp {
                        kind: ScalarLongOpKind::Multiply,
                        lhs: ScalarLongSource::Input(0),
                        rhs: ScalarLongSource::Input(1),
                    },
                    ScalarLongOp {
                        kind: ScalarLongOpKind::Add,
                        lhs: ScalarLongSource::Temporary(0),
                        rhs: ScalarLongSource::Constant(3),
                    },
                ]
                .into_boxed_slice(),
                outputs: [ScalarLongSource::Temporary(1)],
                output_count: 1,
            },
            None,
        );

        let fused = compose_quick_scalar_leaf_program(&arguments, &body).unwrap();
        assert_eq!(fused.operations.len(), 3);
        assert!(matches!(
            fused.operations[1],
            ScalarLongOp {
                kind: ScalarLongOpKind::Multiply,
                lhs: ScalarLongSource::Input(1),
                rhs: ScalarLongSource::Temporary(0),
            }
        ));
        assert!(matches!(
            fused.operations[2],
            ScalarLongOp {
                kind: ScalarLongOpKind::Add,
                lhs: ScalarLongSource::Temporary(1),
                rhs: ScalarLongSource::Constant(3),
            }
        ));
        assert_eq!(fused.outputs[0], ScalarLongSource::Temporary(2));
    }

    #[test]
    fn detects_nested_monomorphic_scalar_method_accumulation() {
        let plan = quick_plan(
            "<?php
class Math {
    public function add($left, $right) { return $left + $right; }
    public function mul($left, $right) { return $left * $right; }
}
$math = new Math();
$sum = 0;
for ($i = 0; $i < 100; $i++) {
    $sum += $math->add($i, $math->mul($i, 2));
}
",
        );
        assert!(matches!(
            plan.term,
            QuickLongTerm::ScalarCallTree {
                argument_count: 2,
                long_input_mask,
                object_input_mask,
                guard,
                do_fcall_ip,
                ..
            } if long_input_mask == 1u64 << 2
                && object_input_mask == 1u64 << 0
                && matches!(guard, ScalarLongCallGuard::MethodCache {
                    receiver_slot: 0,
                    ..
                })
                && do_fcall_ip == guard.cache_ip() + 7
        ));
    }

    #[test]
    fn detects_nested_scalar_function_accumulation_as_call_tree() {
        let plan = quick_plan(
            "<?php
function addNative($left, $right) { return $left + $right; }
function mulNative($left, $right) { return $left * $right; }
$sum = 0;
for ($i = 0; $i < 100; $i++) {
    $sum += addNative($i + 1, mulNative($i, 2));
}
",
        );
        assert!(matches!(
            plan.term,
            QuickLongTerm::ScalarCallTree {
                argument_count: 2,
                long_input_mask,
                object_input_mask: 0,
                guard,
                do_fcall_ip,
                ..
            } if long_input_mask == 1u64 << 1
                && matches!(guard, ScalarLongCallGuard::FunctionCache { .. })
                && do_fcall_ip > guard.cache_ip()
        ));
    }

    #[test]
    fn detects_cold_strict_branch_as_tail_trace_guard() {
        let plan = quick_plan(
            "<?php
function routeStandalone(int $value): int { return ($value * 2) + 1; }
$sum = 0;
for ($i = 0; $i < 100; $i++) {
    $sum += routeStandalone($i);
    if ($i === -1) {
        echo 'never';
    }
}
",
        );
        assert!(matches!(
            plan.tail_guard,
            Some(QuickLongTraceGuard {
                kind: ScalarLongConditionKind::Equal,
                lhs: QuickLongOperand::Slot(lhs),
                rhs: QuickLongOperand::Const(-1),
                expected: false,
                condition_tmp: Some(_),
                resume_ip,
            }) if lhs == plan.induction_cv && resume_ip < plan.increment_ip
        ));
    }

    #[test]
    fn detects_invariant_string_length_as_accumulate_term() {
        for update in ["$i++", "++$i"] {
            let source = format!(
                "<?php
$string = 'abcd';
$sum = 0;
for ($i = 0; $i < 100; {update}) {{
    $sum += strlen($string);
}}
"
            );
            let plan = quick_plan(&source);
            assert_eq!(plan.induction_cv, 2);
            assert_eq!(plan.accumulator_cv, 1);
            assert!(matches!(
                plan.term,
                QuickLongTerm::StringLength { string_cv: 0, .. }
            ));
            assert_eq!(
                plan.increment_kind,
                if update == "++$i" {
                    QuickIncrementKind::Pre
                } else {
                    QuickIncrementKind::Post
                }
            );

            #[cfg(feature = "quick-loops")]
            {
                let main = compile_main(&source);
                assert!(main.op_array.block_plans.iter().any(|plan| matches!(
                    plan,
                    crate::vm::planner::BlockPlan::QuickLongAccumulate(_)
                )));
            }
        }
    }

    #[test]
    fn detects_long_abs_as_accumulate_term() {
        for expression in ["abs($value)", "abs($i)"] {
            let source = format!(
                "<?php
$value = -7;
$sum = 0;
for ($i = 0; $i < 100; ++$i) {{
    $sum += {expression};
}}
"
            );
            let plan = quick_plan(&source);
            assert!(matches!(
                plan.term,
                QuickLongTerm::AbsLong { operand_cv, .. }
                    if operand_cv == if expression == "abs($i)" { 2 } else { 0 }
            ));
        }
    }

    #[test]
    fn detects_prefix_and_postfix_induction_only_loops() {
        let postfix = induction_plan(
            "<?php
$limit = 100;
$i = 0;
while ($i < $limit) {
    $i++;
}
",
        );
        assert!(matches!(postfix.bound, QuickLongBound::Cv(0)));
        assert_eq!(postfix.increment_kind, QuickIncrementKind::Post);

        let prefix = induction_plan(
            "<?php
for ($i = 0; $i < 100; ++$i) {
}
",
        );
        assert!(matches!(prefix.bound, QuickLongBound::Const(100)));
        assert_eq!(prefix.increment_kind, QuickIncrementKind::Pre);

        #[cfg(feature = "quick-loops")]
        {
            let main = compile_main(
                "<?php
for ($i = 0; $i < 100; ++$i) {
}
",
            );
            assert!(
                main.op_array.block_plans.iter().any(|plan| matches!(
                    plan,
                    crate::vm::planner::BlockPlan::QuickLongInduction(_)
                ))
            );
        }
    }

    #[test]
    fn detects_branch_only_if_else_loop_as_typed_ops() {
        let source = "<?php
for ($i = 0; $i < 100; $i++) {
    if ($i == -1) {
    } elseif ($i == -2) {
    } else if ($i == -3) {
    }
}
";
        let plan = long_ops_plan(source);
        assert_eq!(
            plan.ops
                .iter()
                .filter(|op| matches!(op, QuickLongOp::BranchUnlessEq { .. }))
                .count(),
            3
        );
        assert!(matches!(
            plan.ops.last(),
            Some(QuickLongOp::PostIncLoopLt { .. })
        ));

        #[cfg(feature = "quick-loops")]
        {
            let main = compile_main(source);
            assert!(
                main.op_array
                    .block_plans
                    .iter()
                    .any(|plan| matches!(plan, crate::vm::planner::BlockPlan::QuickLongOps(_)))
            );
        }
    }
