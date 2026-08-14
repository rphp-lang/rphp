    fn compile_main(source: &str) -> crate::vm::function::UserFunction {
        let tokens = Lexer::new(source).tokenize().unwrap();
        let statements = Parser::new(tokens).parse().unwrap();
        let result = Compiler::new().compile(&statements).unwrap();
        let has_backward_edge = |op_array: &crate::compiler::OpArray| {
            op_array
                .instructions
                .iter()
                .enumerate()
                .any(|(ip, instruction)| {
                    matches!(instruction.opcode, OpCode::Jmp | OpCode::QuickLongLoopJmp)
                        && (instruction.op1 as usize) < ip
                })
        };
        if has_backward_edge(&result.main) {
            return make_user_function(result.main);
        }
        let function_loop = result
            .functions
            .into_iter()
            .map(|(_, function)| function)
            .find(|function| has_backward_edge(&function.op_array));
        function_loop.unwrap_or_else(|| make_user_function(result.main))
    }

    fn quick_plan(source: &str) -> QuickLongAccumulateLoop {
        let main = compile_main(source);
        if let Some(plan) = main.op_array.block_plans.iter().find_map(|plan| match plan {
            BlockPlan::QuickLongAccumulate(plan) => Some(plan.clone()),
            _ => None,
        }) {
            return plan;
        }
        let selected_backedge = main
            .op_array
            .instructions
            .iter()
            .position(|instruction| instruction.opcode == OpCode::QuickLongLoopJmp);
        #[cfg(feature = "quick-loops")]
        assert!(
            selected_backedge.is_some(),
            "compiler should select a quick loop; instructions: {:#?}",
            main.op_array.instructions
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
        detect_long_accumulate_loop(&main.op_array, header, backedge).unwrap_or_else(|| {
            let plan_kinds = main
                .op_array
                .block_plans
                .iter()
                .map(|plan| match plan {
                    BlockPlan::Interpret => "interpret",
                    BlockPlan::Macro(_) => "macro",
                    BlockPlan::Deoptimized => "deoptimized",
                    BlockPlan::QuickLongAccumulate(_) => "long-accumulate",
                    BlockPlan::QuickDoubleCallAccumulate(_) => "double-call-accumulate",
                    BlockPlan::QuickLongInduction(_) => "long-induction",
                    BlockPlan::QuickForeachLongAccumulate(_) => "foreach-long",
                    BlockPlan::QuickForeachObjectPropertyAccumulate(_) => "foreach-object",
                    BlockPlan::QuickLongOps(_) => "long-ops",
                })
                .collect::<Vec<_>>();
            panic!(
                "selected quick loop must remain structurally detectable; plans: {plan_kinds:?}; instructions: {:#?}",
                main.op_array.instructions
            )
        })
    }

    #[cfg(feature = "quick-loops")]
    #[test]
    fn detects_exact_double_scalar_call_accumulation() {
        let main = compile_main(
            "<?php
function calculateFloat(float $a, float $b, float $c): float {
    return ($a + $b) * $c;
}
function runExactDoubleLoop() {
$scale = 2.0;
$total = 0.0;
for ($i = 0; $i < 100; $i++) {
    $total += calculateFloat(1.5, 2.5, $scale);
}
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
function runDoubleMethodLoop() {
$calculator = new FloatCalculator();
$total = 0.0;
for ($i = 0; $i < 100; $i++) {
    $total += $calculator->calculate(1.5, 2.5, 2.0);
}
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
function runInvariantDoubleLoop() {
$scale = 2.0;
$total = 0.0;
for ($i = 0; $i < 100; $i++) {
    $total += calculateFloat($i * 0.5, $scale + 1.0, 2.0);
}
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
        assert_eq!(plan.string_input_mask, 0);
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
    fn retains_json_source_when_another_operation_consumes_it_as_a_string() {
        let plan = long_ops_plan(
            "<?php
$json = '{\"value\":7}';
$values = ['{\"value\":7}' => 3];
$sum = 0;
for ($i = 0; $i < 100; $i++) {
    $row = json_decode($json, true);
    $key = $json;
    $sum = $sum + $row['value'] + $values[$key];
}
",
        );
        let source = plan
            .typed_invariant_source
            .as_ref()
            .expect("the JSON prelude should remain selected");
        let QuickTypedInvariantProducer::JsonDecodeAssociative {
            input: QuickInvariantInput::StringSlot(input),
        } = source.producer
        else {
            panic!("the source should be the reused JSON CV");
        };
        assert_ne!(plan.string_input_mask & (1u64 << input), 0);
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
function runJsonDoubleLoop() {
$json = '{\"value\":1.25}';
$total = 0.0;
for ($i = 0; $i < 100; $i++) {
    $row = json_decode($json, true);
    $total += scaleJson($row['value']);
}
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
function runGuardedPropertyLoop() {
$tick = new Tick();
$sink = new Sink();
for ($i = 0; $i < 100; $i++) {
    $tick->advance();
    if ($i % 3 == 0) {
        $sink->accept($tick->current());
    }
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
