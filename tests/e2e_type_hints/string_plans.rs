#[test]
fn test_typed_string_return_builds_borrowed_leaf_and_length_consumer() {
    let result = compile_types(
        r#"<?php
function label(int $value): string {
    if (($value & 1) === 0) {
        return 'even';
    }
    return 'odd';
}
function consume(int $value): int {
    $label = label($value);
    return strlen($label) + strlen($label);
}
"#,
    );

    let label = result
        .functions
        .iter()
        .find(|(name, _)| name == "label")
        .map(|(_, function)| function)
        .unwrap();
    assert!(label.scalar_string_plan.is_some());

    let consume = result
        .functions
        .iter()
        .find(|(name, _)| name == "consume")
        .map(|(_, function)| function)
        .unwrap();
    let plan = consume
        .composed_typed_long_plan
        .as_deref()
        .expect("typed string length consumer plan");
    assert!(
        plan.program
            .operations
            .iter()
            .any(|operation| { matches!(operation, ComposedTypedLongOp::StringCall(_)) })
    );
    assert_eq!(
        plan.program
            .operations
            .iter()
            .filter(|operation| matches!(operation, ComposedTypedLongOp::StringLength(_)))
            .count(),
        2
    );
}

#[test]
fn test_typed_string_concat_length_stays_in_borrowed_plan() {
    let result = compile_types(
        r#"<?php
function label(int $value): string {
    if (($value & 1) === 0) {
        return 'even';
    }
    return 'odd';
}
function consume(int $value): int {
    return strlen(label($value) . '!');
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
        .composed_typed_long_plan
        .as_deref()
        .expect("borrowed concat length plan");
    assert!(
        plan.program.operations.iter().any(|operation| {
            matches!(operation, ComposedTypedLongOp::StringConcatLiteral { .. })
        })
    );
}

#[test]
fn test_direct_typed_string_argument_builds_borrowed_length_plan() {
    let result = compile_types(
        r#"<?php
class Scorer {
    public function score(int $value, string $key): int {
        return $value + strlen($key);
    }
}
"#,
    );
    let score = result
        .class_defs
        .iter()
        .find(|class| class.name == "Scorer")
        .unwrap()
        .methods
        .iter()
        .find(|(name, _, _, _, _)| name == "score")
        .map(|(_, _, _, _, function)| function)
        .unwrap();
    let plan = score
        .composed_typed_long_plan
        .as_deref()
        .expect("direct borrowed String input plan");
    assert_eq!(plan.long_argument_mask, 1);
    assert_eq!(plan.string_argument_mask, 2);
    assert!(plan.program.operations.iter().any(|operation| matches!(
        operation,
        ComposedTypedLongOp::StringLength(ScalarStringSource::Input(1))
    )));
}

#[test]
fn test_mixed_string_use_builds_an_exact_borrowed_input_plan() {
    let result = compile_types(
        r#"<?php
function mixedScore(int $value, mixed $key): int {
    return $value + strlen($key);
}
function ambiguousMixed(mixed $value): int {
    return strlen($value) + $value;
}
"#,
    );
    let mixed_score = result
        .functions
        .iter()
        .find(|(name, _)| name == "mixedScore")
        .map(|(_, function)| function)
        .unwrap();
    let plan = mixed_score
        .composed_typed_long_plan
        .as_deref()
        .expect("semantic String use should refine the broad mixed signature");
    assert_eq!(plan.long_argument_mask, 1);
    assert_eq!(plan.string_argument_mask, 2);
    assert!(plan.program.operations.iter().any(|operation| matches!(
        operation,
        ComposedTypedLongOp::StringLength(ScalarStringSource::Input(1))
    )));

    let ambiguous = result
        .functions
        .iter()
        .find(|(name, _)| name == "ambiguousMixed")
        .map(|(_, function)| function)
        .unwrap();
    assert!(ambiguous.composed_typed_long_plan.is_none());
}

#[test]
fn test_scalar_local_alias_keeps_parameter_mutation_in_canonical_vm() {
    let result = compile_types(
        r#"<?php
function localAlias(int $value): int {
    $local = $value;
    $local = $local + 1;
    return $local;
}
function parameterMutation(int $value): int {
    $value = $value + 1;
    return $value;
}
"#,
    );

    let local_alias = result
        .functions
        .iter()
        .find(|(name, _)| name == "localAlias")
        .map(|(_, function)| function)
        .unwrap();
    let parameter_mutation = result
        .functions
        .iter()
        .find(|(name, _)| name == "parameterMutation")
        .map(|(_, function)| function)
        .unwrap();
    assert!(local_alias.scalar_long_plan.is_some());
    assert!(parameter_mutation.scalar_long_plan.is_none());
    assert_eq!(
        run_php(
            r#"<?php
function localAlias(int $value): int {
    $local = $value;
    $local = $local + 1;
    return $local;
}
function parameterMutation(int $value): int {
    $value = $value + 1;
    return $value;
}
echo localAlias(4) . ":" . parameterMutation(4);
"#
        ),
        "5:5"
    );
}

#[test]
fn test_scalar_modulo_guard_side_exits_to_catchable_division_by_zero_error() {
    assert_eq!(
        run_php(
        r#"<?php
function invalidModulo(int $value): int {
    return ($value % 0) ^ 13;
}
try {
    invalidModulo(4);
} catch (DivisionByZeroError $error) {
    echo get_class($error), ':', $error->getMessage();
}
"#,
        ),
        "DivisionByZeroError:Modulo by zero",
    );
}

#[test]
fn test_return_only_int_signature_uses_guarded_scalar_plan() {
    let result = compile_types(
        r#"<?php
function returnOnly($value): int {
    return (($value * 3) + 1) % 1000003;
}
"#,
    );
    let function = &result.functions[0].1;
    assert!(function.scalar_long_plan.is_some());
}
