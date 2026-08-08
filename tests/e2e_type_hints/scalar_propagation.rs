// ── Declaration-derived scalar propagation ──

#[test]
fn test_exact_int_return_flows_into_caller_bytecode() {
    let result = compile_types(
        r#"<?php
function source(int $value): int { return $value % 97; }
function consume(int $value): int { return (source($value) % 13) ^ 3; }
"#,
    );
    let consume = &result
        .functions
        .iter()
        .find(|(name, _)| name == "consume")
        .unwrap()
        .1
        .op_array;

    assert!(consume.instructions.iter().any(|instruction| {
        instruction.opcode == OpCode::DoFcall
            && instruction.known_result_type() == KnownScalarType::Long
            && instruction._pad & CALL_FLAG_EXACT_SCALAR_ARGS != 0
    }));
    assert!(
        consume
            .instructions
            .iter()
            .any(|instruction| instruction.opcode == OpCode::Mod_LongLong)
    );
    assert!(
        consume
            .instructions
            .iter()
            .any(|instruction| instruction.opcode == OpCode::BitwiseXor_LongLong)
    );
}

#[test]
fn test_exact_string_return_flows_through_concat_and_strlen() {
    let result = compile_types(
        r#"<?php
function source(string $value): string { return $value; }
function consume(string $value): int { return strlen(source($value) . "!"); }
"#,
    );
    let consume = &result
        .functions
        .iter()
        .find(|(name, _)| name == "consume")
        .unwrap()
        .1
        .op_array;

    assert!(consume.instructions.iter().any(|instruction| {
        instruction.opcode == OpCode::DoFcall
            && instruction.known_result_type() == KnownScalarType::String
    }));
    assert!(
        consume
            .instructions
            .iter()
            .any(|instruction| instruction.opcode == OpCode::Concat_StringString)
    );
    assert!(
        consume
            .instructions
            .iter()
            .any(|instruction| instruction.opcode == OpCode::Strlen_String)
    );
}

#[test]
fn test_mutable_typed_parameter_stays_on_guarded_strlen() {
    let result = compile_types(
        r#"<?php
function consume(int $value, bool $change): int {
    if ($change) { $value = "changed"; }
    return strlen($value);
}
"#,
    );
    let consume = &result.functions[0].1.op_array;
    assert!(
        !consume
            .instructions
            .iter()
            .any(|instruction| instruction.opcode == OpCode::Strlen_String)
    );
}

#[test]
fn test_unknown_argument_keeps_runtime_typed_call_guard() {
    let result = compile_types(
        r#"<?php
function target(int $value): int { return $value; }
function forward($value): int { return target($value); }
"#,
    );
    let forward = &result
        .functions
        .iter()
        .find(|(name, _)| name == "forward")
        .unwrap()
        .1
        .op_array;
    assert!(forward.instructions.iter().any(|instruction| {
        instruction.opcode == OpCode::DoFcall && instruction._pad & CALL_FLAG_EXACT_SCALAR_ARGS == 0
    }));
}

#[test]
fn test_propagated_int_and_string_operations_preserve_results() {
    assert_eq!(
        run_php(
            r#"<?php
function sourceInt(int $value): int { return $value % 97; }
function consumeInt(int $value): int { return (sourceInt($value) % 13) ^ 3; }
function sourceString(string $value): string { return $value; }
function consumeString(string $value): int { return strlen(sourceString($value) . "!"); }
echo consumeInt(12345);
echo ":";
echo consumeString("typed");
"#
        ),
        "3:6"
    );
}

#[test]
fn test_bad_declared_return_never_reaches_unguarded_consumer() {
    assert_eq!(
        run_php(
            r#"<?php
declare(strict_types=1);
function source(): int { return "bad"; }
function consume(): int { return source() % 7; }
try { consume(); } catch (TypeError $error) { echo "caught"; }
"#
        ),
        "caught"
    );
}

#[test]
fn test_proven_long_modulo_handles_integer_minimum() {
    assert_eq!(
        run_php(
            r#"<?php
function remainder(int $left, int $right): int { return $left % $right; }
echo remainder(PHP_INT_MIN, -1);
"#
        ),
        "0"
    );
}

#[test]
fn test_proven_long_addition_still_validates_overflowed_return() {
    assert_eq!(
        run_php(
            r#"<?php
function add(int $left, int $right): int { return $left + $right; }
try { add(PHP_INT_MAX, 1); } catch (TypeError $error) { echo "caught"; }
"#
        ),
        "caught"
    );
}

#[test]
fn test_method_return_contract_selects_one_dispatch_guard_and_scalar_consumers() {
    let result = compile_types(
        r#"<?php
class Source {
    function value(int $value): int {
        if (($value & 1) === 0) { return $value + 3; }
        return $value - 2;
    }
    function label(int $value): string {
        if (($value & 1) === 0) { return "even"; }
        return "odd";
    }
}
function consumeInt(Source $source, int $value): int {
    $result = $source->value($value);
    return ($result % 97) ^ 3;
}
function consumeString(Source $source, int $value): int {
    return strlen($source->label($value));
}
"#,
    );
    let consume_int = &result
        .functions
        .iter()
        .find(|(name, _)| name == "consumeInt")
        .unwrap()
        .1
        .op_array;
    let guarded_init = consume_int
        .instructions
        .iter()
        .find(|instruction| instruction.opcode == OpCode::InitMethodCall)
        .unwrap();
    assert_eq!(
        guarded_init.method_return_guard_type(),
        KnownScalarType::Long
    );
    assert!(guarded_init.has_method_long_args_guard());
    assert!(
        consume_int
            .instructions
            .iter()
            .any(|instruction| instruction.opcode == OpCode::Mod_LongLong)
    );
    assert!(consume_int.instructions.iter().any(|instruction| {
        instruction.opcode == OpCode::DoFcall && instruction._pad & CALL_FLAG_EXACT_SCALAR_ARGS != 0
    }));
    assert!(
        consume_int
            .instructions
            .iter()
            .any(|instruction| instruction.opcode == OpCode::BitwiseXor_LongLong)
    );

    let consume_string = &result
        .functions
        .iter()
        .find(|(name, _)| name == "consumeString")
        .unwrap()
        .1
        .op_array;
    let string_init = consume_string
        .instructions
        .iter()
        .find(|instruction| instruction.opcode == OpCode::InitMethodCall)
        .unwrap();
    assert_eq!(
        string_init.method_return_guard_type(),
        KnownScalarType::String
    );
    assert!(
        consume_string
            .instructions
            .iter()
            .any(|instruction| instruction.opcode == OpCode::Strlen_String)
    );

    assert_eq!(
        run_php(
            r#"<?php
class RuntimeSource {
    function value(int $value): int { return $value + 2; }
    function label(int $value): string { return "typed"; }
}
function runtimeConsume(RuntimeSource $source, int $value): int {
    return ($source->value($value) % 7) + strlen($source->label($value));
}
echo runtimeConsume(new RuntimeSource(), 5);
"#
        ),
        "5"
    );
}

#[test]
fn test_polymorphic_method_return_dispatch_accepts_compatible_override() {
    assert_eq!(
        run_php(
            r#"<?php
class IntegerSource { function value($value): int { return $value + 2; } }
class ShiftedSource extends IntegerSource { function value($value): int { return $value + 4; } }
function consume(IntegerSource $source, $value) { return $source->value($value) + 1; }
$integer = new IntegerSource();
$shifted = new ShiftedSource();
for ($i = 0; $i < 20; $i++) {
    consume($integer, $i);
    consume($shifted, $i);
}
echo consume($integer, 4);
echo ":";
echo consume($shifted, 4);
"#
        ),
        "7:9"
    );
}

#[test]
fn test_bad_typed_method_return_throws_before_guarded_consumer() {
    assert_eq!(
        run_php(
            r#"<?php
declare(strict_types=1);
class BadSource { function value(): int { return "bad"; } }
function consume(BadSource $source) { return $source->value() % 7; }
try { consume(new BadSource()); } catch (TypeError $error) { echo "caught"; }
"#
        ),
        "caught"
    );
}

#[test]
fn test_nullsafe_and_reference_receivers_do_not_use_method_return_guard() {
    let result = compile_types(
        r#"<?php
class Source { function label(): string { return "value"; } }
function nullable(?Source $source): int { return strlen($source?->label()); }
function referenced(Source &$source): int { return strlen($source->label()); }
"#,
    );
    for name in ["nullable", "referenced"] {
        let function = &result
            .functions
            .iter()
            .find(|(candidate, _)| candidate == name)
            .unwrap()
            .1
            .op_array;
        assert!(function.instructions.iter().all(|instruction| {
            instruction.opcode != OpCode::InitMethodCall
                || instruction.method_return_guard_type() == KnownScalarType::Unknown
        }));
        assert!(
            function
                .instructions
                .iter()
                .all(|instruction| { instruction.opcode != OpCode::Strlen_String })
        );
    }
}

#[test]
fn test_method_contract_flows_from_new_this_and_inheritance() {
    let result = compile_types(
        r#"<?php
class Source {
    function value(): int { return 42; }
    function fromThis(): int { return $this->value() % 5; }
}
class Child extends Source {}
class UntypedChild extends Source {
    function value() { return 42.5; }
}
function fromNew(): int {
    $source = new Source();
    return $source->value() % 5;
}
function fromInherited(Child $source): int {
    return $source->value() % 5;
}
function fromUntypedOverride(UntypedChild $source) {
    return $source->value() % 5;
}
"#,
    );

    for name in ["fromNew", "fromInherited"] {
        let function = &result
            .functions
            .iter()
            .find(|(candidate, _)| candidate == name)
            .unwrap()
            .1
            .op_array;
        assert!(
            function
                .instructions
                .iter()
                .any(|instruction| instruction.opcode == OpCode::Mod_LongLong)
        );
    }

    let source = result
        .class_defs
        .iter()
        .find(|class| class.name == "Source")
        .unwrap();
    let from_this = &source
        .methods
        .iter()
        .find(|(name, _, _, _, _)| name == "fromThis")
        .unwrap()
        .4
        .op_array;
    assert!(
        from_this
            .instructions
            .iter()
            .any(|instruction| instruction.opcode == OpCode::Mod_LongLong)
    );

    let untyped = &result
        .functions
        .iter()
        .find(|(name, _)| name == "fromUntypedOverride")
        .unwrap()
        .1
        .op_array;
    assert!(untyped.instructions.iter().all(|instruction| {
        instruction.opcode != OpCode::InitMethodCall
            || instruction.method_return_guard_type() == KnownScalarType::Unknown
    }));
    assert!(
        untyped
            .instructions
            .iter()
            .all(|instruction| instruction.opcode != OpCode::Mod_LongLong)
    );
}

#[test]
fn test_conditional_scalar_plan_is_compiled_for_function_and_method() {
    let result = compile_types(
        r#"<?php
function choose(int $value): int {
    if (($value & 1) === 0) {
        return $value + 3;
    }
    return $value - 2;
}
class Selector {
    function choose(int $value): int {
        if ($value < 10) {
            return $value * 2;
        } else {
            return $value - 4;
        }
    }
}
"#,
    );

    let function = result
        .functions
        .iter()
        .find(|(name, _)| name == "choose")
        .map(|(_, function)| function)
        .unwrap();
    assert!(
        function
            .scalar_long_plan
            .as_ref()
            .is_some_and(|plan| plan.select.is_some())
    );

    let method = &result.class_defs[0].methods[0].4;
    assert!(
        method
            .scalar_long_plan
            .as_ref()
            .is_some_and(|plan| plan.select.is_some())
    );
}

#[cfg(feature = "quick-loops")]
#[test]
fn test_intdiv_conditional_method_composes_into_quick_scalar_loop() {
    let source = include_str!("../../benches/corpus_typed_ledger_pipeline.php");
    let result = compile_types(source);
    let fee = result
        .class_defs
        .iter()
        .find(|class| class.name.eq_ignore_ascii_case("TypedLedgerFeePolicy"))
        .and_then(|class| {
            class
                .methods
                .iter()
                .find(|(name, ..)| name.eq_ignore_ascii_case("fee"))
        })
        .map(|method| &method.4)
        .expect("fee method");
    let scalar_plan = fee.scalar_long_plan.as_deref().expect("scalar fee plan");
    assert!(scalar_plan.select.is_some());
    assert!(
        scalar_plan
            .program
            .operations
            .iter()
            .any(|operation| operation.kind == ScalarLongOpKind::IntDivide)
    );

    let total = result
        .functions
        .iter()
        .find(|(name, _)| name.eq_ignore_ascii_case("runTypedLedgerPipeline"))
        .map(|(_, function)| function)
        .expect("typed ledger function");
    assert!(total.op_array.block_plans.iter().any(|block| {
        matches!(
            block,
            BlockPlan::QuickLongOps(plan)
                if plan.ops.iter().any(|operation| {
                    matches!(
                        operation,
                        QuickLongOp::ScalarMethodCall {
                            call: QuickTypedMethodCall {
                                argument_count: 2,
                                ..
                            },
                            ..
                        }
                    )
                })
        )
    }));
}

#[test]
fn test_conditional_scalar_plan_preserves_both_control_flow_edges() {
    assert_eq!(
        run_php(
            r#"<?php
function masked(int $value): int {
    if (($value & 1) === 0) {
        return $value + 3;
    }
    return $value - 2;
}
class Selector {
    function choose(int $value): int {
        if ($value < 10) {
            return $value * 2;
        } else {
            return $value - 4;
        }
    }
}
$selector = new Selector();
echo masked(8) . ":" . masked(9) . ":";
echo $selector->choose(7) . ":" . $selector->choose(20);
"#
        ),
        "11:7:14:16"
    );
}

#[test]
fn test_conditional_scalar_plan_falls_back_without_evaluating_inactive_arm() {
    assert_eq!(
        run_php(
            r#"<?php
function weak($value) {
    if ($value === 0) {
        return 3;
    }
    return $value - 2;
}
function overflow(int $value): int {
    if ($value === 9223372036854775807) {
        return 7;
    }
    return $value + 1;
}
echo weak(5.0) . ":" . overflow(9223372036854775807);
"#
        ),
        "3:7"
    );
}

#[test]
fn test_composed_scalar_plan_tracks_local_aliases_modulo_and_xor() {
    let result = compile_types(
        r#"<?php
function source(int $value): int {
    if (($value & 1) === 0) {
        return $value + 3;
    }
    return $value - 2;
}
function consume(int $value): int {
    $local = source($value);
    return ($local % 97) ^ 13;
}
"#,
    );

    let source = result
        .functions
        .iter()
        .find(|(name, _)| name == "source")
        .map(|(_, function)| function)
        .unwrap();
    let consume = result
        .functions
        .iter()
        .find(|(name, _)| name == "consume")
        .map(|(_, function)| function)
        .unwrap();
    assert!(source.scalar_long_plan.is_some());
    assert!(consume.composed_scalar_long_plan.is_some());
}

#[test]
fn test_composed_scalar_plan_separates_typed_object_receiver_from_long_arguments() {
    let result = compile_types(
        r#"<?php
class Source {
    public function value(int $value): int {
        if (($value & 1) === 0) {
            return $value + 3;
        }
        return $value - 2;
    }
}
function consume(Source $source, int $value): int {
    $local = $source->value($value);
    return ($local % 97) ^ 13;
}
"#,
    );

    let consume = result
        .functions
        .iter()
        .find(|(name, _)| name == "consume")
        .map(|(_, function)| function)
        .unwrap();
    let plan = consume
        .composed_scalar_long_plan
        .as_deref()
        .expect("mixed object/long composed scalar plan");
    assert_eq!(plan.public_args, 2);
    assert_eq!(plan.object_argument_mask, 0b01);
    assert_eq!(plan.long_argument_mask, 0b10);
    assert!(plan.program.operations.iter().any(|operation| matches!(
        operation,
        ComposedScalarLongOp::Call(call)
            if matches!(call.guard, ScalarLongCallGuard::MethodCache { .. })
    )));
}
