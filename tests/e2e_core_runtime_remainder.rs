mod common;

use common::run_php;

#[test]
fn non_numeric_special_float_spellings_do_not_enter_arithmetic() {
    assert_eq!(
        run_php(
            r#"<?php
foreach (["inf", "infinity", "nan"] as $value) {
    try { var_dump($value + 1); }
    catch (Throwable $error) { echo $error->getMessage(), "\n"; }
}
"#,
        ),
        "Unsupported operand types: string + int\nUnsupported operand types: string + int\nUnsupported operand types: string + int\n"
    );
}

#[test]
fn error_suppression_covers_the_complete_include_operation() {
    assert_eq!(
        run_php(
            r#"<?php
var_dump(@include __DIR__ . '/definitely-missing-core-runtime-remainder.php');
echo "after\n";
"#,
        ),
        "bool(false)\nafter\n"
    );
}

#[test]
fn inherited_private_static_access_names_the_requested_scope() {
    assert_eq!(
        run_php(
            r#"<?php
class StaticPrivateParent { private static $slot; }
class StaticPrivateChild extends StaticPrivateParent {
    static function probe() {
        try { return self::$slot; }
        catch (Throwable $error) { echo $error->getMessage(), "\n"; }
    }
}
StaticPrivateChild::probe();
"#,
        ),
        "Cannot access private property StaticPrivateChild::$slot\n"
    );
}

#[test]
fn numeric_date_grammar_rejects_components_above_its_lexical_range() {
    assert_eq!(
        run_php(
            r#"<?php
foreach (["9999-11-33", "2020-13-01"] as $value) {
    try { new DateTime($value); }
    catch (Throwable $error) {
        echo get_class($error), ": ", $error->getMessage(), "\n";
    }
}
"#,
        ),
        "DateMalformedStringException: Failed to parse time string (9999-11-33) at position 9 (3): Unexpected character\nDateMalformedStringException: Failed to parse time string (2020-13-01) at position 6 (3): Unexpected character\n"
    );
}

#[test]
fn eval_in_a_method_inherits_the_compatible_live_receiver() {
    assert_eq!(
        run_php(
            r#"<?php
class EvalReceiverParent {
    public function label() { return $this->name; }
}
class EvalReceiverChild extends EvalReceiverParent {
    public $name = 'kept receiver';
    public function probe() { return eval('return EvalReceiverParent::label();'); }
}
var_dump((new EvalReceiverChild)->probe());
"#,
        ),
        "string(13) \"kept receiver\"\n"
    );
}

#[test]
fn dynamic_member_method_name_errors_remain_catchable() {
    assert_eq!(
        run_php(
            r#"<?php
class DynamicMethodProbe {}
$probe = new DynamicMethodProbe();
try { $probe->{0}(); }
catch (Throwable $error) { echo get_class($error), ': ', $error->getMessage(), "\n"; }
"#,
        ),
        "Error: Method name must be a string\n"
    );
}

#[test]
fn immutable_date_mutator_reports_its_native_no_discard_contract() {
    assert_eq!(
        run_php(
            r#"<?php
$date = new DateTimeImmutable('now');
$date->setTimestamp(0);
"#,
        ),
        "\nWarning: The return value of method DateTimeImmutable::setTimestamp() should either be used or intentionally ignored by casting it as (void), as DateTimeImmutable::setTimestamp() does not modify the object itself in <main> on line 3\n"
    );
}
