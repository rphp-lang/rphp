#[cfg(test)]
mod tests {
    use super::*;
    use crate::compiler::compile::Compiler;
    use crate::compiler::make_user_function;
    use crate::lexer::Lexer;
    use crate::parser::Parser;
    use crate::vm::function::{
        ComposedScalarDoubleFunctionPlan, ComposedScalarDoubleOp, ScalarDoubleCall,
        ScalarDoubleFunctionPlan, ScalarDoubleOp, ScalarDoubleOpKind, ScalarDoubleProgram,
        ScalarDoubleSelect, ScalarDoubleSource, ScalarLongConditionKind,
    };
    use crate::vm::planner::BlockPlan;

    fn dynamic_double_argument(register: u8) -> QuickDoubleArgumentProgram {
        QuickDoubleArgumentProgram {
            operations: vec![QuickDoubleArgumentOp {
                kind: ScalarDoubleOpKind::Add,
                lhs: QuickDoubleSource::Induction,
                rhs: QuickDoubleSource::Constant(0.5),
            }]
            .into_boxed_slice(),
            outputs: [
                QuickDoubleSource::Temporary(register),
                QuickDoubleSource::Constant(0.0),
                QuickDoubleSource::Constant(0.0),
                QuickDoubleSource::Constant(0.0),
                QuickDoubleSource::Constant(0.0),
                QuickDoubleSource::Constant(0.0),
                QuickDoubleSource::Constant(0.0),
                QuickDoubleSource::Constant(0.0),
            ],
            output_count: 1,
            input_slots: [u16::MAX; 8],
            input_count: 0,
        }
    }

    #[test]
    fn forwards_dynamic_double_argument_used_before_register_overwrite() {
        let arguments = dynamic_double_argument(0);
        let leaf = ScalarDoubleFunctionPlan::new(
            1,
            ScalarDoubleProgram {
                operations: vec![ScalarDoubleOp {
                    kind: ScalarDoubleOpKind::Add,
                    lhs: ScalarDoubleSource::Input(0),
                    rhs: ScalarDoubleSource::Constant(1.0),
                }]
                .into_boxed_slice(),
                output: ScalarDoubleSource::Temporary(0),
            },
        );

        assert_eq!(arguments.register_forwardable_output_mask(&leaf), 1);
    }

    #[test]
    fn retains_buffer_when_x86_rhs_would_be_overwritten() {
        let arguments = dynamic_double_argument(0);
        let leaf = ScalarDoubleFunctionPlan::new(
            1,
            ScalarDoubleProgram {
                operations: vec![ScalarDoubleOp {
                    kind: ScalarDoubleOpKind::Subtract,
                    lhs: ScalarDoubleSource::Constant(10.0),
                    rhs: ScalarDoubleSource::Input(0),
                }]
                .into_boxed_slice(),
                output: ScalarDoubleSource::Temporary(0),
            },
        );

        assert_eq!(arguments.register_forwardable_output_mask(&leaf), 0);
    }

    #[test]
    fn flattens_guarded_double_leaf_with_target_neutral_source_remapping() {
        let composed = ComposedScalarDoubleFunctionPlan {
            public_args: 1,
            operations: vec![
                ComposedScalarDoubleOp::Call(ScalarDoubleCall {
                    guard: ScalarLongCallGuard::FunctionCache { cache_ip: 0 },
                    arguments: vec![ScalarDoubleSource::Input(0)].into_boxed_slice(),
                }),
                ComposedScalarDoubleOp::Arithmetic(ScalarDoubleOp {
                    kind: ScalarDoubleOpKind::Add,
                    lhs: ScalarDoubleSource::Temporary(0),
                    rhs: ScalarDoubleSource::Constant(3.0),
                }),
            ]
            .into_boxed_slice(),
            output: ScalarDoubleSource::Temporary(1),
        };
        let leaf = ScalarDoubleFunctionPlan::new(
            1,
            ScalarDoubleProgram {
                operations: vec![ScalarDoubleOp {
                    kind: ScalarDoubleOpKind::Multiply,
                    lhs: ScalarDoubleSource::Input(0),
                    rhs: ScalarDoubleSource::Constant(2.0),
                }]
                .into_boxed_slice(),
                output: ScalarDoubleSource::Temporary(0),
            },
        );

        let flattened = compose_scalar_double_program(
            &composed,
            &[
                Some(ResolvedScalarDoubleProgram {
                    public_args: leaf.public_args,
                    program: &leaf.program,
                    select: leaf.select,
                }),
                None,
            ],
        )
        .unwrap();
        assert_eq!(flattened.program.operations.len(), 2);
        assert!(matches!(
            flattened.program.operations[0],
            ScalarDoubleOp {
                kind: ScalarDoubleOpKind::Multiply,
                lhs: ScalarDoubleSource::Input(0),
                rhs: ScalarDoubleSource::Constant(2.0),
            }
        ));
        assert!(matches!(
            flattened.program.operations[1],
            ScalarDoubleOp {
                kind: ScalarDoubleOpKind::Add,
                lhs: ScalarDoubleSource::Temporary(0),
                rhs: ScalarDoubleSource::Constant(3.0),
            }
        ));
        assert_eq!(flattened.program.output, ScalarDoubleSource::Temporary(1));
        assert!(flattened.select.is_none());
    }

    fn conditional_double_leaf() -> ScalarDoubleFunctionPlan {
        ScalarDoubleFunctionPlan::new_conditional(
            2,
            ScalarDoubleProgram {
                operations: vec![
                    ScalarDoubleOp {
                        kind: ScalarDoubleOpKind::Multiply,
                        lhs: ScalarDoubleSource::Input(0),
                        rhs: ScalarDoubleSource::Constant(1.5),
                    },
                    ScalarDoubleOp {
                        kind: ScalarDoubleOpKind::Subtract,
                        lhs: ScalarDoubleSource::Input(0),
                        rhs: ScalarDoubleSource::Constant(1.0),
                    },
                ]
                .into_boxed_slice(),
                output: ScalarDoubleSource::Temporary(0),
            },
            ScalarDoubleSelect {
                kind: ScalarLongConditionKind::LessThan,
                lhs: ScalarDoubleSource::Input(0),
                rhs: ScalarDoubleSource::Input(1),
                shared_operation_count: 0,
                when_true_operation_count: 1,
                when_false_operation_count: 1,
                when_true: ScalarDoubleSource::Temporary(0),
                when_false: ScalarDoubleSource::Temporary(1),
                merge_result: false,
            },
        )
    }

    #[test]
    fn flattens_one_conditional_double_leaf_into_a_common_suffix() {
        let composed = ComposedScalarDoubleFunctionPlan {
            public_args: 2,
            operations: vec![
                ComposedScalarDoubleOp::Call(ScalarDoubleCall {
                    guard: ScalarLongCallGuard::FunctionCache { cache_ip: 0 },
                    arguments: vec![ScalarDoubleSource::Input(0), ScalarDoubleSource::Input(1)]
                        .into_boxed_slice(),
                }),
                ComposedScalarDoubleOp::Arithmetic(ScalarDoubleOp {
                    kind: ScalarDoubleOpKind::Add,
                    lhs: ScalarDoubleSource::Temporary(0),
                    rhs: ScalarDoubleSource::Constant(3.0),
                }),
            ]
            .into_boxed_slice(),
            output: ScalarDoubleSource::Temporary(1),
        };
        let leaf = conditional_double_leaf();

        let flattened = compose_scalar_double_program(
            &composed,
            &[
                Some(ResolvedScalarDoubleProgram {
                    public_args: leaf.public_args,
                    program: &leaf.program,
                    select: leaf.select,
                }),
                None,
            ],
        )
        .expect("one conditional callee should flatten");

        let select = flattened.select.expect("flattened merge select");
        assert!(select.merge_result);
        assert_eq!(select.operation_ranges(3), Some((0, 1, 2)));
        assert_eq!(select.when_true, ScalarDoubleSource::Temporary(0));
        assert_eq!(select.when_false, ScalarDoubleSource::Temporary(1));
        assert!(matches!(
            flattened.program.operations[2],
            ScalarDoubleOp {
                kind: ScalarDoubleOpKind::Add,
                lhs: ScalarDoubleSource::Selection,
                rhs: ScalarDoubleSource::Constant(3.0),
            }
        ));
        assert_eq!(flattened.program.output, ScalarDoubleSource::Temporary(2));
    }

    #[test]
    fn rejects_two_conditional_double_callees_from_one_flattened_region() {
        let composed = ComposedScalarDoubleFunctionPlan {
            public_args: 2,
            operations: vec![
                ComposedScalarDoubleOp::Call(ScalarDoubleCall {
                    guard: ScalarLongCallGuard::FunctionCache { cache_ip: 0 },
                    arguments: vec![ScalarDoubleSource::Input(0), ScalarDoubleSource::Input(1)]
                        .into_boxed_slice(),
                }),
                ComposedScalarDoubleOp::Call(ScalarDoubleCall {
                    guard: ScalarLongCallGuard::FunctionCache { cache_ip: 1 },
                    arguments: vec![ScalarDoubleSource::Input(0), ScalarDoubleSource::Input(1)]
                        .into_boxed_slice(),
                }),
                ComposedScalarDoubleOp::Arithmetic(ScalarDoubleOp {
                    kind: ScalarDoubleOpKind::Add,
                    lhs: ScalarDoubleSource::Temporary(0),
                    rhs: ScalarDoubleSource::Temporary(1),
                }),
            ]
            .into_boxed_slice(),
            output: ScalarDoubleSource::Temporary(2),
        };
        let leaf = conditional_double_leaf();
        let resolved = ResolvedScalarDoubleProgram {
            public_args: leaf.public_args,
            program: &leaf.program,
            select: leaf.select,
        };

        assert!(
            compose_scalar_double_program(&composed, &[Some(resolved), Some(resolved), None],)
                .is_none()
        );
    }

    #[test]
    fn rejects_flattened_double_body_beyond_shared_register_capacity() {
        let composed = ComposedScalarDoubleFunctionPlan {
            public_args: 1,
            operations: vec![
                ComposedScalarDoubleOp::Call(ScalarDoubleCall {
                    guard: ScalarLongCallGuard::FunctionCache { cache_ip: 0 },
                    arguments: vec![ScalarDoubleSource::Input(0)].into_boxed_slice(),
                }),
                ComposedScalarDoubleOp::Arithmetic(ScalarDoubleOp {
                    kind: ScalarDoubleOpKind::Add,
                    lhs: ScalarDoubleSource::Temporary(0),
                    rhs: ScalarDoubleSource::Constant(1.0),
                }),
            ]
            .into_boxed_slice(),
            output: ScalarDoubleSource::Temporary(1),
        };
        let mut leaf_operations = Vec::new();
        for index in 0..8 {
            leaf_operations.push(ScalarDoubleOp {
                kind: ScalarDoubleOpKind::Add,
                lhs: if index == 0 {
                    ScalarDoubleSource::Input(0)
                } else {
                    ScalarDoubleSource::Temporary(index - 1)
                },
                rhs: ScalarDoubleSource::Constant(1.0),
            });
        }
        let leaf = ScalarDoubleFunctionPlan::new(
            1,
            ScalarDoubleProgram {
                operations: leaf_operations.into_boxed_slice(),
                output: ScalarDoubleSource::Temporary(7),
            },
        );

        assert!(
            compose_scalar_double_program(
                &composed,
                &[
                    Some(ResolvedScalarDoubleProgram {
                        public_args: leaf.public_args,
                        program: &leaf.program,
                        select: leaf.select,
                    }),
                    None,
                ],
            )
            .is_none()
        );
    }

    fn compile_main(source: &str) -> crate::vm::function::UserFunction {
        let tokens = Lexer::new(source).tokenize().unwrap();
        let statements = Parser::new(tokens).parse().unwrap();
        let result = Compiler::new().compile(&statements).unwrap();
        make_user_function(result.main)
    }

    fn quick_plan(source: &str) -> QuickLongAccumulateLoop {
        let main = compile_main(source);
        let selected_backedge = main
            .op_array
            .instructions
            .iter()
            .position(|instruction| instruction.opcode == OpCode::QuickLongLoopJmp);
        #[cfg(feature = "quick-loops")]
        assert!(
            selected_backedge.is_some(),
            "compiler should select a quick loop"
        );
        let backedge = selected_backedge
            .or_else(|| {
                main.op_array
                    .instructions
                    .iter()
                    .enumerate()
                    .position(|(ip, instruction)| {
                        instruction.opcode == OpCode::Jmp && (instruction.op1 as usize) < ip
                    })
            })
            .expect("source should contain a backward edge");
        let header = main.op_array.instructions[backedge].op1 as usize;
        detect_long_accumulate_loop(&main.op_array, header, backedge).unwrap()
    }

    #[cfg(feature = "quick-loops")]
    #[test]
    fn detects_exact_double_scalar_call_accumulation() {
        let main = compile_main(
            "<?php
function calculateFloat(float $a, float $b, float $c): float {
    return ($a + $b) * $c;
}
$scale = 2.0;
$total = 0.0;
for ($i = 0; $i < 100; $i++) {
    $total += calculateFloat(1.5, 2.5, $scale);
}
",
        );
        let plan = main
            .op_array
            .block_plans
            .iter()
            .find_map(|plan| match plan {
                BlockPlan::QuickDoubleCallAccumulate(plan) => Some(plan),
                _ => None,
            })
            .expect("compiler should select the Double call-accumulate loop");
        assert_eq!(plan.argument_program.output_count, 3);
        assert_eq!(plan.argument_program.input_count, 1);
        assert_eq!(plan.argument_program.input_slots[0], 0);
        assert!(matches!(
            plan.argument_program.outputs[0],
            QuickDoubleSource::Constant(1.5)
        ));
        assert!(matches!(
            plan.argument_program.outputs[1],
            QuickDoubleSource::Constant(2.5)
        ));
        assert!(matches!(
            plan.argument_program.outputs[2],
            QuickDoubleSource::Input(0)
        ));
        assert_eq!(plan.accumulator_cv, 1);
        assert_eq!(plan.induction_cv, 2);
    }

    #[cfg(feature = "quick-loops")]
    #[test]
    fn detects_monomorphic_double_method_accumulation() {
        let main = compile_main(
            "<?php
class FloatCalculator {
    public function calculate(float $a, float $b, float $c): float {
        return (($a + $b) * $c) - 2.0;
    }
}
$calculator = new FloatCalculator();
$total = 0.0;
for ($i = 0; $i < 100; $i++) {
    $total += $calculator->calculate(1.5, 2.5, 2.0);
}
",
        );
        let plan = main
            .op_array
            .block_plans
            .iter()
            .find_map(|plan| match plan {
                BlockPlan::QuickDoubleCallAccumulate(plan) => Some(plan),
                _ => None,
            })
            .expect("compiler should select the Double method/accumulate loop");
        assert!(matches!(
            plan.guard,
            ScalarLongCallGuard::MethodCache {
                receiver_slot: 0,
                ..
            }
        ));
        assert_eq!(plan.argument_program.output_count, 3);
        assert_eq!(plan.argument_program.input_count, 0);
        assert_eq!(
            plan.argument_program.outputs[0],
            QuickDoubleSource::Constant(1.5)
        );
        assert_eq!(
            plan.argument_program.outputs[1],
            QuickDoubleSource::Constant(2.5)
        );
        assert_eq!(
            plan.argument_program.outputs[2],
            QuickDoubleSource::Constant(2.0)
        );
        assert_eq!(plan.accumulator_cv, 1);
        assert_eq!(plan.induction_cv, 2);
    }

    #[cfg(feature = "quick-loops")]
    #[test]
    fn detects_induction_and_invariant_double_argument_expressions() {
        let main = compile_main(
            "<?php
function calculateFloat(float $a, float $b, float $c): float {
    return (($a + $b) * $c) - 2.0;
}
$scale = 2.0;
$total = 0.0;
for ($i = 0; $i < 100; $i++) {
    $total += calculateFloat($i * 0.5, $scale + 1.0, 2.0);
}
",
        );
        let plan = main
            .op_array
            .block_plans
            .iter()
            .find_map(|plan| match plan {
                BlockPlan::QuickDoubleCallAccumulate(plan) => Some(plan),
                _ => None,
            })
            .expect("compiler should retain scalar argument expressions");
        let arguments = &plan.argument_program;
        assert_eq!(arguments.operations.len(), 2);
        assert_eq!(arguments.input_count, 1);
        assert_eq!(arguments.input_slots[0], 0);
        assert_eq!(arguments.operations[0].kind, ScalarDoubleOpKind::Multiply);
        assert_eq!(arguments.operations[0].lhs, QuickDoubleSource::Induction);
        assert_eq!(
            arguments.operations[0].rhs,
            QuickDoubleSource::Constant(0.5)
        );
        assert_eq!(arguments.operations[1].kind, ScalarDoubleOpKind::Add);
        assert_eq!(arguments.operations[1].lhs, QuickDoubleSource::Input(0));
        assert_eq!(
            arguments.operations[1].rhs,
            QuickDoubleSource::Constant(1.0)
        );
        assert_eq!(arguments.outputs[0], QuickDoubleSource::Temporary(0));
        assert_eq!(arguments.outputs[1], QuickDoubleSource::Temporary(1));
        assert_eq!(arguments.outputs[2], QuickDoubleSource::Constant(2.0));
    }

    fn induction_plan(source: &str) -> QuickLongInductionLoop {
        let main = compile_main(source);
        main.op_array
            .instructions
            .iter()
            .enumerate()
            .filter(|(ip, instruction)| {
                matches!(instruction.opcode, OpCode::Jmp | OpCode::QuickLongLoopJmp)
                    && (instruction.op1 as usize) < *ip
            })
            .find_map(|(backedge, instruction)| {
                detect_long_induction_loop(&main.op_array, instruction.op1 as usize, backedge)
            })
            .expect("source should contain an induction-only quick loop")
    }

    fn long_ops_plan(source: &str) -> QuickLongOpsLoop {
        let main = compile_main(source);
        main.op_array
            .instructions
            .iter()
            .enumerate()
            .filter(|(ip, instruction)| {
                matches!(instruction.opcode, OpCode::Jmp | OpCode::QuickLongLoopJmp)
                    && (instruction.op1 as usize) < *ip
            })
            .find_map(|(backedge, instruction)| {
                detect_long_ops_loop(&main.op_array, instruction.op1 as usize, backedge)
            })
            .unwrap_or_else(|| {
                panic!(
                    "source should contain a typed long ops loop; instructions: {:#?}",
                    main.op_array.instructions
                )
            })
    }

    #[test]
    fn detects_invariant_json_decode_long_projections() {
        let plan = long_ops_plan(
            "<?php
$json = '{\"age\":30,\"scores\":[95,87]}';
$sum = 0;
for ($i = 0; $i < 100; $i++) {
    $row = json_decode($json, true);
    $sum = $sum + $row['age'] + $row['scores'][0] + $row['scores'][1];
}
",
        );
        let source = plan
            .typed_invariant_source
            .as_ref()
            .expect("stable associative json_decode should become a prelude");
        assert_eq!(source.projections.len(), 3);
        assert_eq!(source.long_output_mask.count_ones(), 3);
        assert!(
            plan.ops
                .iter()
                .any(|operation| matches!(operation, QuickLongOp::JsonProjectionStep { .. }))
        );
        assert!(source.projections.iter().any(|output| {
            matches!(
                output.path.as_ref(),
                [QuickInvariantPathElement::StringLiteral(_)]
            )
        }));
        assert!(source.projections.iter().any(|output| {
            matches!(
                output.path.as_ref(),
                [
                    QuickInvariantPathElement::StringLiteral(_),
                    QuickInvariantPathElement::Integer(0)
                ]
            )
        }));
    }

    #[test]
    fn derives_invariant_string_length_as_a_long_projection() {
        let plan = long_ops_plan(
            "<?php
$json = '{\"name\":\"hyper-optimized\"}';
$sum = 0;
for ($i = 0; $i < 100; $i++) {
    $row = json_decode($json, true);
    $sum = $sum + strlen($row['name']);
}
",
        );
        let source = plan
            .typed_invariant_source
            .as_ref()
            .expect("fixed string projection should become a typed prelude");
        assert_eq!(source.string_output_mask.count_ones(), 1);
        assert_eq!(source.long_output_mask.count_ones(), 1);
        assert!(
            source
                .projections
                .iter()
                .any(|projection| projection.kind == QuickInvariantValueKind::String)
        );
        assert!(
            source
                .projections
                .iter()
                .any(|projection| projection.kind == QuickInvariantValueKind::StringLength)
        );
    }

    #[cfg(feature = "quick-loops")]
    #[test]
    fn feeds_invariant_json_double_projection_into_scalar_call_ir() {
        let main = compile_main(
            "<?php
function scaleJson(float $value): float {
    return $value * 1.5;
}
$json = '{\"value\":1.25}';
$total = 0.0;
for ($i = 0; $i < 100; $i++) {
    $row = json_decode($json, true);
    $total += scaleJson($row['value']);
}
",
        );
        let plan = main
            .op_array
            .block_plans
            .iter()
            .find_map(|plan| match plan {
                BlockPlan::QuickDoubleCallAccumulate(plan) => Some(plan),
                _ => None,
            })
            .unwrap_or_else(|| {
                panic!(
                    "compiler should select a typed Double source; instructions: {:#?}",
                    main.op_array.instructions
                )
            });
        let source = plan
            .typed_invariant_source
            .as_ref()
            .expect("associative JSON source should be retained");
        assert_eq!(source.double_output_mask.count_ones(), 1);
        assert_eq!(source.projections.len(), 1);
        assert_eq!(source.projections[0].kind, QuickInvariantValueKind::Double);
        assert_eq!(plan.argument_program.input_count, 1);
        assert_eq!(
            plan.argument_program.input_slots[0],
            source.projections[0].result
        );
    }

    #[test]
    #[cfg(feature = "quick-loops")]
    fn selects_straight_array_application_region_from_general_typed_ops() {
        let main = compile_main(
            "<?php
$row = ['a' => 2, 'b' => 3, 'c' => 4];
$a = 10;
$b = 20;
$c = 30;
$a = $a + $row['a'];
$b = $b + $row['b'];
$c = $c + $row['c'];
echo $a + $b + $c;
",
        );
        let (entry_ip, entry) = main
            .op_array
            .instructions
            .iter()
            .enumerate()
            .find(|(_, instruction)| {
                instruction.opcode == OpCode::FetchDimR && instruction.extended_value != 0
            })
            .expect("compiler should mark a straight typed region entry");
        let block_idx = entry.extended_value as usize - 1;
        let BlockPlan::QuickLongOps(plan) = &main.op_array.block_plans[block_idx] else {
            panic!("marked entry must reference a typed region plan");
        };
        assert_eq!(plan.header_ip, entry_ip);
        assert!(plan.straight_array_kernel.is_some());

        let first_fetch_result = entry.result;
        assert_eq!(
            plan.long_input_mask & (1u64 << first_fetch_result),
            0,
            "a temporary produced inside the region is not an entry input"
        );
    }

    #[test]
    fn detects_guarded_property_calls_inside_general_long_ops_loop() {
        let plan = long_ops_plan(
            "<?php
class Tick {
    public $value = 0;
    public function advance() { $this->value = $this->value + 1; }
    public function current() { return $this->value; }
}
class Sink {
    public $value = 0;
    public function accept($value) { $this->value = $this->value + $value; }
}
$tick = new Tick();
$sink = new Sink();
for ($i = 0; $i < 100; $i++) {
    $tick->advance();
    if ($i % 3 == 0) {
        $sink->accept($tick->current());
    }
}
",
        );
        assert_eq!(plan.object_input_mask, (1u64 << 0) | (1u64 << 1));
        assert!(plan.ops.iter().any(|operation| matches!(
            operation,
            QuickLongOp::PropertyMethodCall {
                call: QuickTypedMethodCall {
                    argument_count: 0,
                    ..
                },
                ..
            }
        )));
        assert!(
            plan.ops
                .iter()
                .any(|operation| matches!(operation, QuickLongOp::ComposedPropertyCall { .. }))
        );
    }

    #[test]
    fn detects_guarded_invariant_object_property_reads_in_long_ops_loop() {
        let plan = long_ops_plan(
            "<?php
$row = json_decode('{\"value\":11,\"name\":\"alpha\"}');
$sum = 0;
for ($i = 0; $i < 100; $i++) {
    $sum += $row->value + strlen($row->name);
}
",
        );
        assert_ne!(plan.object_input_mask, 0);
        assert!(
            plan.ops
                .iter()
                .any(|operation| matches!(operation, QuickLongOp::ObjectPropertyLong { .. }))
        );
        assert!(
            plan.ops.iter().any(|operation| matches!(
                operation,
                QuickLongOp::ObjectPropertyStringLength { .. }
            ))
        );
    }

    fn foreach_long_accumulate_plan(source: &str) -> QuickForeachLongAccumulateLoop {
        let main = compile_main(source);
        main.op_array
            .instructions
            .iter()
            .enumerate()
            .filter(|(ip, instruction)| {
                matches!(instruction.opcode, OpCode::Jmp | OpCode::QuickLongLoopJmp)
                    && (instruction.op1 as usize) < *ip
            })
            .find_map(|(backedge, instruction)| {
                detect_foreach_long_accumulate_loop(
                    &main.op_array,
                    instruction.op1 as usize,
                    backedge,
                )
            })
            .unwrap_or_else(|| {
                panic!(
                    "source should contain a foreach long accumulation loop; instructions: {:#?}",
                    main.op_array.instructions
                )
            })
    }

    #[test]
    fn detects_value_only_foreach_long_accumulation() {
        let source = "<?php
$values = [1, 2, 3, 4];
$sum = 0;
foreach ($values as $value) {
    $sum += $value;
}
";
        let plan = foreach_long_accumulate_plan(source);
        assert_eq!(plan.accumulator_cv, 1);
        assert_eq!(plan.value_cv, 2);
        assert_eq!(plan.sum_ip, plan.header_ip + 2);
        assert_eq!(plan.exit_ip, plan.header_ip + 5);

        #[cfg(feature = "quick-loops")]
        {
            let main = compile_main(source);
            assert!(main.op_array.block_plans.iter().any(|plan| matches!(
                plan,
                crate::vm::planner::BlockPlan::QuickForeachLongAccumulate(_)
            )));
        }
    }

    #[test]
    fn rejects_key_value_foreach_long_accumulation() {
        let main = compile_main(
            "<?php
$values = [1, 2, 3, 4];
$sum = 0;
foreach ($values as $key => $value) {
    $sum += $value;
}
",
        );
        assert!(
            main.op_array
                .instructions
                .iter()
                .enumerate()
                .filter(|(ip, instruction)| {
                    matches!(instruction.opcode, OpCode::Jmp | OpCode::QuickLongLoopJmp)
                        && (instruction.op1 as usize) < *ip
                })
                .all(|(backedge, instruction)| {
                    detect_foreach_long_accumulate_loop(
                        &main.op_array,
                        instruction.op1 as usize,
                        backedge,
                    )
                    .is_none()
                })
        );
    }

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
        assert_eq!(
            plan.array_output_mask
                & (plan.long_input_mask
                    | plan.long_output_mask
                    | plan.bool_output_mask
                    | plan.array_input_mask),
            0
        );
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
        }
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
$mixer = new Mixer();
$values = ['left' => 0, 'right' => 0];
$key = 'left';
$accepted = 0;
$needle = -1;
for ($i = 0; $i < 100; $i++) {
    if (($i % 2) == 0) { $key = 'right'; } else { $key = 'left'; }
    $score = $mixer->score($i, $key);
    $values[$key] = $values[$key] + $score;
    $isAccepted = $mixer->accepted($score, $i);
    $accepted = $accepted + $isAccepted;
    if ($i === $needle) { echo 'never'; }
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
        assert_eq!(
            plan.ops
                .iter()
                .filter(|op| matches!(op, QuickLongOp::AssignStringSlot { .. }))
                .count(),
            2
        );
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
}
