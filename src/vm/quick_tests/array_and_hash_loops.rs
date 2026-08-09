    #[test]
    fn detects_packed_array_index_as_accumulate_term() {
        let plan = quick_plan(
            "<?php
$values = [1, 2, 3, 4];
$sum = 0;
for ($i = 0; $i < 4; $i++) {
    $sum += $values[$i];
}
",
        );
        assert!(matches!(
            plan.term,
            QuickLongTerm::ArrayIndex {
                array_cv: 0,
                index: QuickArrayIndex::Long(QuickLongOperand::Slot(2)),
                ..
            }
        ));
    }

    #[test]
    fn detects_string_literal_array_index_as_accumulate_term() {
        let plan = quick_plan(
            "<?php
$values = ['hot' => 7];
$sum = 0;
for ($i = 0; $i < 100; $i++) {
    $sum += $values['hot'];
}
",
        );
        assert!(matches!(
            plan.term,
            QuickLongTerm::ArrayIndex {
                array_cv: 0,
                index: QuickArrayIndex::StringLiteral(_),
                ..
            }
        ));
    }

    #[test]
    fn detects_invariant_value_slot_array_index_as_accumulate_term() {
        for body in [
            "$sum += $values[$key];",
            "$value = $values[$key];\n    $sum += $value;",
        ] {
            let plan = quick_plan(&format!(
                "<?php
$values = ['hot' => 7];
$key = 'hot';
$sum = 0;
$value = 0;
for ($i = 0; $i < 100; $i++) {{
    {body}
}}
"
            ));
            assert!(matches!(
                plan.term,
                QuickLongTerm::ArrayIndex {
                    index: QuickArrayIndex::ValueSlot(1),
                    ..
                }
            ));
        }
    }

    #[test]
    fn detects_materialized_invariant_array_index_as_accumulate_term() {
        for index in ["'hot'", "7"] {
            let plan = quick_plan(&format!(
                "<?php
$values = ['hot' => 7, 7 => 9];
$sum = 0;
$value = 0;
for ($i = 0; $i < 100; $i++) {{
    $value = $values[{index}];
    $sum += $value;
}}
"
            ));
            assert!(matches!(
                plan.term,
                QuickLongTerm::ArrayIndex {
                    array_cv: 0,
                    index: QuickArrayIndex::StringLiteral(_)
                        | QuickArrayIndex::Long(QuickLongOperand::Const(7)),
                    destination: Some(2),
                    ..
                }
            ));
        }
    }

    #[test]
    fn detects_direct_accumulation_with_constant_bound() {
        let plan = quick_plan(
            "<?php
$sum = 0;
for ($i = 0; $i < 100; $i++) {
    $sum += $i;
}
",
        );
        assert_eq!(plan.induction_cv, 1);
        assert_eq!(plan.accumulator_cv, 0);
        assert!(matches!(plan.bound, QuickLongBound::Const(100)));
        assert!(matches!(plan.term, QuickLongTerm::Induction));
        assert_eq!(plan.condition_tmp, None);
    }

    #[test]
    fn detects_two_cv_nested_term_as_typed_ops() {
        let plan = long_ops_plan(
            "<?php
$sum = 0;
for ($i = 0; $i < 10; $i++) {
    for ($j = 0; $j < 20; $j++) {
        $sum += $i + $j;
    }
}
",
        );
        assert_eq!(plan.ops.len(), 3);
        assert_eq!(
            plan.ops
                .iter()
                .filter(|op| matches!(op, QuickLongOp::AddAddAssign { .. }))
                .count(),
            1
        );
        assert!(matches!(
            plan.ops.first(),
            Some(QuickLongOp::BranchUnlessLt {
                rhs: QuickLongOperand::Const(20),
                ..
            })
        ));
    }

    #[test]
    fn detects_long_array_push_as_typed_op() {
        let plan = long_ops_plan(
            "<?php
$values = [];
for ($i = 0; $i < 100; $i++) {
    $values[] = $i;
}
",
        );
        assert!(matches!(
            plan.ops.as_slice(),
            [
                QuickLongOp::BranchUnlessLt { .. },
                QuickLongOp::ArrayPushLong {
                    value: QuickLongOperand::Slot(_),
                    ..
                },
                QuickLongOp::PostIncLoopLt { .. },
            ]
        ));
        assert_ne!(plan.array_output_mask, 0);
        assert_eq!(plan.structural_array_output_mask, plan.array_output_mask);
        assert_eq!(
            plan.array_output_mask
                & (plan.long_input_mask
                    | plan.long_output_mask
                    | plan.bool_output_mask
                    | plan.array_input_mask),
            0
        );
        #[cfg(feature = "quick-loops")]
        assert!(crate::vm::execute::quick_array_push_loop_kernel(&plan).is_some());
    }

    #[test]
    fn leaves_multi_operation_array_push_loops_in_typed_dispatch() {
        let plan = long_ops_plan(
            "<?php
$values = [];
for ($i = 0; $i < 100; $i++) {
    $values[] = $i;
    $values[] = $i;
}
",
        );
        assert!(matches!(
            plan.ops.as_slice(),
            [
                QuickLongOp::BranchUnlessLt { .. },
                QuickLongOp::ArrayPushLong { .. },
                QuickLongOp::ArrayPushLong { .. },
                QuickLongOp::PostIncLoopLt { .. },
            ]
        ));
        #[cfg(feature = "quick-loops")]
        assert!(crate::vm::execute::quick_array_push_loop_kernel(&plan).is_none());
    }

    #[test]
    fn detects_structural_integer_array_set_as_typed_op() {
        let plan = long_ops_plan(
            "<?php
$values = [];
for ($i = 0; $i < 100; $i++) {
    $key = (($i * 17) & 255) + 1000;
    $values[$key] = $i;
}
",
        );
        assert!(plan.ops.iter().any(|operation| matches!(
            operation,
            QuickLongOp::SetArrayLong {
                index: QuickLongOperand::Slot(_),
                ..
            }
        )));
        assert_ne!(plan.array_output_mask, 0);
        assert_eq!(plan.array_output_mask & plan.array_input_mask, 0);
    }

    #[test]
    fn detects_wrapping_shift_before_structural_integer_array_set() {
        let plan = long_ops_plan(
            "<?php
$values = [];
for ($i = 0; $i < 100; $i++) {
    $key = ($i << 32) | (($i * $i) & 1048575);
    $values[$key] = $i;
}
",
        );
        assert!(plan.ops.iter().any(|operation| matches!(
            operation,
            QuickLongOp::Shift {
                left: true,
                rhs: QuickLongOperand::Const(32),
                ..
            }
        )));
        assert!(plan
            .ops
            .iter()
            .any(|operation| matches!(operation, QuickLongOp::SetArrayLong { .. })));
    }

    #[test]
    fn rejects_structural_array_set_with_borrowed_read_view() {
        let main = compile_main(
            "<?php
$values = [0 => 1];
$sum = 0;
for ($i = 0; $i < 100; $i++) {
    $sum = $sum + $values[0];
    $key = $i + 1000;
    $values[$key] = $i;
}
",
        );
        let plan = main
            .op_array
            .instructions
            .iter()
            .enumerate()
            .filter(|(ip, instruction)| {
                matches!(instruction.opcode, OpCode::Jmp | OpCode::QuickLongLoopJmp)
                    && (instruction.op1 as usize) < *ip
            })
            .find_map(|(backedge, instruction)| {
                detect_long_ops_loop(&main.op_array, instruction.op1 as usize, backedge)
            });
        assert!(plan.is_none());
    }

    #[test]
    fn detects_literal_and_invariant_string_append_as_typed_ops() {
        for (setup, expression, expected_slot) in
            [("", "'x'", None), ("$suffix = 'yz';", "$suffix", Some(0))]
        {
            let plan = long_ops_plan(&format!(
                "<?php
{setup}
$value = '';
for ($i = 0; $i < 100; $i++) {{
    $value .= {expression};
}}
"
            ));
            assert!(matches!(
                plan.ops.as_slice(),
                [
                    QuickLongOp::BranchUnlessLt { .. },
                    QuickLongOp::StringAppend { source, .. },
                    QuickLongOp::PostIncLoopLt { .. },
                ] if match expected_slot {
                    Some(slot) => *source == QuickStringAppendSource::Slot(slot),
                    None => matches!(source, QuickStringAppendSource::Literal(_)),
                }
            ));
            assert_ne!(plan.string_append_mask, 0);
            assert_eq!(plan.string_append_mask & plan.string_input_mask, 0);
            #[cfg(feature = "quick-loops")]
            assert!(crate::vm::execute::quick_string_append_loop_kernel(&plan).is_some());
        }
    }

    #[test]
    fn leaves_multi_operation_string_append_loops_in_typed_dispatch() {
        let plan = long_ops_plan(
            "<?php
$value = '';
for ($i = 0; $i < 100; $i++) {
    $value .= 'x';
    $value .= 'y';
}
",
        );
        assert!(matches!(
            plan.ops.as_slice(),
            [
                QuickLongOp::BranchUnlessLt { .. },
                QuickLongOp::StringAppend { .. },
                QuickLongOp::StringAppend { .. },
                QuickLongOp::PostIncLoopLt { .. },
            ]
        ));
        #[cfg(feature = "quick-loops")]
        assert!(crate::vm::execute::quick_string_append_loop_kernel(&plan).is_none());
    }

    #[test]
    fn detects_materialized_array_long_read_as_selected_typed_ops() {
        let source = "<?php
$values = [1, 2, 3, 4];
$sum = 0;
for ($i = 0; $i < 4; $i++) {
    $value = $values[$i];
    $sum += $value;
}
";
        let plan = long_ops_plan(source);
        assert!(plan.ops.iter().any(|op| matches!(
            op,
            QuickLongOp::FetchArrayLong {
                destination: Some(_),
                ..
            }
        )));
        assert!(matches!(
            plan.ops.as_slice(),
            [
                QuickLongOp::BranchUnlessLt { .. },
                QuickLongOp::FetchArrayLong { .. },
                QuickLongOp::AddAssign { .. },
                QuickLongOp::PostIncLoopLt { .. },
            ]
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
        assert_ne!(plan.array_input_mask, 0);
        assert_eq!(
            plan.array_input_mask
                & (plan.long_input_mask | plan.long_output_mask | plan.bool_output_mask),
            0
        );
    }

    #[test]
    fn fuses_general_binary_results_materialized_into_loop_cvs() {
        let plan = long_ops_plan(
            "<?php
$last = 0;
$product = 0;
for ($i = 0; $i < 100; $i++) {
    $last = 20 + ($i % 400);
    $product = $i * 73;
}
echo $last + $product;
",
        );
        assert_eq!(
            plan.ops
                .iter()
                .filter(|operation| matches!(operation, QuickLongOp::BinaryAssign { .. }))
                .count(),
            2,
            "{:#?}",
            plan.ops
        );
    }

    #[test]
    fn detects_string_literal_hash_read_as_typed_op() {
        let plan = long_ops_plan(
            "<?php
$values = ['hot' => 7];
$sum = 0;
$last = 0;
for ($i = 0; $i < 100; $i++) {
    $last = $values['hot'];
    $sum += $i;
}
",
        );
        assert!(plan.ops.iter().any(|op| matches!(
            op,
            QuickLongOp::FetchArrayLong {
                index: QuickArrayIndex::StringLiteral(_),
                ..
            }
        )));
        assert_ne!(plan.array_input_mask, 0);
    }

    #[test]
    fn detects_strided_integer_hash_scan_as_typed_ops() {
        let plan = long_ops_plan(
            "<?php
$values = [100 => 3, 107 => 5, 114 => 7];
$stride = 7;
$key = 100;
$sum = 0;
for ($i = 0; $i < 3; $i++) {
    $sum += $values[$key];
    $key = $key + $stride;
}
",
        );
        assert_eq!(plan.ops.len(), 5, "{:#?}", plan.ops);
        assert!(matches!(
            plan.ops.as_slice(),
            [
                QuickLongOp::BranchUnlessLt { .. },
                QuickLongOp::FetchArrayLong {
                    index: QuickArrayIndex::Long(QuickLongOperand::Slot(_)),
                    ..
                },
                QuickLongOp::AddAssign { .. },
                QuickLongOp::AddAssign { .. },
                QuickLongOp::PostIncLoopLt { .. },
            ]
        ));
    }

    #[test]
    fn detects_composed_bitwise_integer_hash_key_as_typed_ops() {
        let plan = long_ops_plan(
            "<?php
$values = [1000000 => 3, 1104515245 => 5];
$sum = 0;
for ($i = 0; $i < 2; $i++) {
    $key = (($i * 1103515245) & 2147483647) + 1000000;
    $sum += $values[$key];
}
",
        );
        assert!(
            plan.ops.iter().any(|operation| matches!(
                operation,
                QuickLongOp::Binary {
                    kind: ScalarLongOpKind::BitwiseAnd,
                    ..
                }
            )),
            "{:#?}",
            plan.ops
        );
        assert!(
            plan.ops.iter().any(|operation| matches!(
                operation,
                QuickLongOp::FetchArrayLong {
                    index: QuickArrayIndex::Long(QuickLongOperand::Slot(_)),
                    ..
                }
            )),
            "{:#?}",
            plan.ops
        );
        assert!(
            plan.ops
                .iter()
                .any(|operation| matches!(operation, QuickLongOp::AddAssign { .. })),
            "{:#?}",
            plan.ops
        );
    }

    #[test]
    fn detects_materialized_hash_value_with_two_aggregates_as_typed_ops() {
        let plan = long_ops_plan(
            "<?php
$values = [100 => 3, 107 => 5, 114 => 7];
$key = 100;
$sum = 0;
$adjusted = 0;
$one = 1;
$stride = 7;
for ($i = 0; $i < 3; $i++) {
    $value = $values[$key];
    $sum += $value;
    $adjusted += $value + $one;
    $key = $key + $stride;
}
",
        );
        assert_eq!(
            plan.ops
                .iter()
                .filter(|op| matches!(op, QuickLongOp::FetchArrayLong { .. }))
                .count(),
            1
        );
        assert!(plan.ops.iter().any(|op| matches!(
            op,
            QuickLongOp::FetchArrayLong {
                destination: Some(_),
                ..
            }
        )));
    }

    #[test]
    fn detects_filtered_hash_aggregate_as_typed_ops() {
        let plan = long_ops_plan(
            "<?php
$values = [100 => 3, 107 => 5, 114 => 7];
$key = 100;
$sum = 0;
$stride = 7;
for ($i = 0; $i < 3; $i++) {
    $value = $values[$key];
    if ($value < 6) {
        $sum += $value;
    }
    $key = $key + $stride;
}
",
        );
        assert_eq!(
            plan.ops
                .iter()
                .filter(|op| matches!(op, QuickLongOp::FetchArrayLong { .. }))
                .count(),
            1
        );
        assert!(
            plan.ops
                .iter()
                .any(|op| matches!(op, QuickLongOp::BranchUnlessLt { .. }))
        );
        assert!(plan.ops.iter().any(|op| matches!(
            op,
            QuickLongOp::FetchArrayLong {
                destination: Some(_),
                ..
            }
        )));
        assert!(
            plan.ops
                .iter()
                .any(|op| matches!(op, QuickLongOp::ConditionalAddAssign { .. }))
        );
    }
