mod common;

use common::run_php;

#[test]
fn surplus_argument_owners_retire_after_declared_arguments_and_locals() {
    assert_eq!(
        run_php(
            r#"<?php
ini_set('zend.exception_ignore_args', '1');
class OrderedOwner {
    function __construct(public string $id) {}
    function __destruct() { echo $this->id, '|'; }
}
function extras(OrderedOwner $argument) {
    $local = new OrderedOwner('local');
    echo 'body|';
}
extras(new OrderedOwner('argument'), new OrderedOwner('extra1'), new OrderedOwner('extra2'));
echo 'end';
"#
        ),
        "body|argument|local|extra1|extra2|end"
    );
}

#[test]
fn a_surplus_argument_destructor_runs_before_the_caller_observes_return() {
    assert_eq!(
        run_php(
            r#"<?php
ini_set('zend.exception_ignore_args', '1');
class ExtraOwner {
    function __destruct() { echo 'drop|'; throw new LogicException('extra'); }
}

function extras() { echo 'body|'; }
try { extras(new ExtraOwner); echo 'wrong|'; }
catch (LogicException $error) { echo $error->getMessage(), '|'; }
echo 'end';
"#
        ),
        "body|drop|extra|end"
    );
}

#[test]
fn surplus_argument_aliases_retire_after_finally_when_body_exception_leaves() {
    assert_eq!(
        run_php(
            r#"<?php
ini_set('zend.exception_ignore_args', '1');
class ExtraOwner {
    function __destruct() { echo 'drop|'; throw new LogicException('extra'); }
}
function extras() {
    $local = func_get_arg(0);
    try { throw new RuntimeException('body'); }
    finally { echo 'finally|'; }
}
try { extras(new ExtraOwner); }
catch (Throwable $error) {
    echo $error::class, ':', $error->getMessage(), ':', $error->getPrevious()?->getMessage(), '|';
}
echo 'end';
"#
        ),
        "finally|drop|LogicException:extra:body|end"
    );
}

const STRICT_STORAGE: &str = r#"<?php
declare(strict_types=1);
function describe(?float $x, int|float $y, ?array $values = null): string {
    return gettype($x) . ':' . gettype($y) . ':' . gettype($values);
}
for ($i = 0; $i < 100; $i++) { describe(1.5, $i, []); }
echo describe(3, 7, []), '|', describe(null, 7.5), '|';
try { describe('bad', 7); } catch (TypeError $e) { echo 'rejected|'; }
function widen(?float &$value): void { echo gettype($value), '|'; }
$number = 8;
widen($number);
echo gettype($number), '|';
function either(int|float &$value): void { echo gettype($value); }
$integer = 9;
either($integer);
"#;

#[test]
fn composite_arguments_keep_exact_storage_and_reference_widening() {
    assert_eq!(
        run_php(STRICT_STORAGE),
        "double:integer:array|NULL:double:NULL|rejected|double|double|integer"
    );
}

const WEAK_CONVERSION: &str = r#"<?php
$conversions = 0;
class TextValue {
    function __toString(): string {
        $GLOBALS['conversions']++;
        return 'converted';
    }
}
function describeWeak(float|string $value): string { return gettype($value) . ':' . $value; }
function maybeNumber(?float $value): string { return gettype($value) . ':' . $value; }
for ($i = 0; $i < 100; $i++) { describeWeak('text'); }
echo describeWeak(3), '|', describeWeak('4'), '|', describeWeak(new TextValue), '|';
echo maybeNumber('5'), '|', $conversions;
"#;

#[test]
fn composite_guard_leaves_weak_conversion_to_one_canonical_call() {
    assert_eq!(
        run_php(WEAK_CONVERSION),
        "double:3|string:4|string:converted|double:5|1"
    );
}

const INTERSECTION_SCOPE: &str = r#"<?php
declare(strict_types=1);
interface LeftContract {}
interface RightContract {}
class BothContracts implements LeftContract, RightContract {}
class OnlyLeft implements LeftContract {}
$entered = 0;
function bothOrNull((LeftContract&RightContract)|null $value): string {
    $GLOBALS['entered']++;
    return $value === null ? 'null' : get_class($value);
}
$both = new BothContracts;
for ($i = 0; $i < 100; $i++) { bothOrNull($both); }
echo bothOrNull(null), '|', bothOrNull($both), '|';
try { bothOrNull(new OnlyLeft); } catch (TypeError $e) { echo 'rejected|'; }
class_alias('BothContracts', 'BothAlias');
echo bothOrNull(new BothAlias), '|', $entered, '|';
class BaseValue {}
trait RelativeTypes {
    function relatives(?self $same, parent|string $base): string {
        return ($same === null ? 'null' : get_class($same)) . ':' . gettype($base);
    }
}
class DerivedValue extends BaseValue { use RelativeTypes; }
class OtherValue extends BaseValue {}
$derived = new DerivedValue;
for ($i = 0; $i < 100; $i++) { $derived->relatives($derived, new BaseValue); }
echo $derived->relatives(null, 'text'), '|', $derived->relatives($derived, new BaseValue), '|';
try { $derived->relatives(new OtherValue, 'text'); } catch (TypeError $e) { echo 'scope'; }
"#;

#[test]
fn composite_class_checks_revalidate_intersections_aliases_and_trait_scope() {
    assert_eq!(
        run_php(INTERSECTION_SCOPE),
        "null|BothContracts|rejected|BothContracts|103|null:string|DerivedValue:object|scope"
    );
}

const BINDING_EFFECTS: &str = r#"<?php
declare(strict_types=1);
$entered = 0;
class Marker {
    function __destruct() { echo 'destroy|'; }
}
function bindValues(?Marker $first = null, int|string $second = 1): string {
    $GLOBALS['entered']++;
    return ($first === null ? 'null' : 'marker') . ':' . $second;
}
for ($i = 0; $i < 100; $i++) { bindValues(); }
echo bindValues(second: 'named'), '|', bindValues(new Marker, second: 2), '|';
try { bindValues(second: []); } catch (TypeError $e) { echo 'argument|'; }
function requiredValues(?Marker $first, int|string $second): void {
    $GLOBALS['entered']++;
}
try { requiredValues(second: 3); } catch (ArgumentCountError $e) { echo 'missing|'; }
function wrongReturn(?Marker $value): int {
    $GLOBALS['entered']++;
    return 'bad';
}
try { wrongReturn(null); } catch (TypeError $e) { echo 'return|'; }
echo $entered;
"#;

#[test]
fn composite_calls_keep_binding_errors_cleanup_and_body_effects() {
    assert_eq!(
        run_php(BINDING_EFFECTS),
        "null:named|destroy|marker:2|argument|missing|return|103"
    );
}

const FULL_BOUNDARIES: &str = r#"<?php
function invokeMaybe(?callable $callback): void {
    if ($callback !== null) { $callback(); }
}
invokeMaybe(null);
invokeMaybe(function () { echo 'callback|'; });
function variadicValues(int|string ...$values): void { echo count($values), '|'; }
variadicValues(1, 'two');
function generate(?int $value) {
    try { yield $value; } finally { echo 'finally|'; }
}
$generator = generate(3);
echo $generator->current(), '|';
$generator->next();
echo 'done';
"#;

#[test]
fn callable_variadic_and_generator_boundaries_remain_canonical() {
    assert_eq!(run_php(FULL_BOUNDARIES), "callback|2|3|finally|done");
}

#[test]
fn consumed_argument_owner_is_released_when_user_type_validation_fails() {
    assert_eq!(
        run_php(
            r#"<?php
ini_set('zend.exception_ignore_args', '1');
class Marker { function __destruct() { echo 'destroy|'; } }
function integerOnly(int $value): void {}
try { integerOnly(new Marker); } catch (TypeError $e) { echo 'catch|'; }
echo 'end';
"#
        ),
        "destroy|catch|end"
    );
}

#[test]
fn consumed_argument_owner_is_released_when_internal_validation_fails() {
    assert_eq!(
        run_php(
            r#"<?php
ini_set('zend.exception_ignore_args', '1');
class Marker { function __destruct() { echo 'destroy|'; } }
try { strlen(new Marker); } catch (TypeError $e) { echo 'catch|'; }
echo 'end';
"#
        ),
        "destroy|catch|end"
    );
}

#[test]
fn completed_internal_call_retires_its_argument_before_publishing_the_result() {
    assert_eq!(
        run_php(
            r#"<?php
class Payload { function __destruct() { echo 'destroy|'; } }
echo count([WeakReference::create(new Payload)]), '|end';
"#
        ),
        "destroy|1|end"
    );
}

#[test]
fn completed_internal_argument_destructor_can_replace_result_observation() {
    assert_eq!(
        run_php(
            r#"<?php
ini_set('zend.exception_ignore_args', '1');
class Payload {
    function __destruct() { echo 'destroy|'; throw new LogicException('release'); }
}
try { $weak = WeakReference::create(new Payload); echo 'wrong|'; }
catch (LogicException $e) { echo 'catch|', isset($weak) ? 'set' : 'unset'; }
echo '|end';
"#
        ),
        "destroy|catch|unset|end"
    );
}

#[test]
fn arity_failures_retire_consumed_arguments_before_the_catch() {
    for call in [
        "function accept($first, $second): void {} try { accept(new Payload); }",
        "try { strlen(new Payload, 2); }",
        "function accept($first, $second, $third): void {} try { accept(new Payload, third: 3); }",
    ] {
        let source = format!(
            "<?php ini_set('zend.exception_ignore_args', '1'); \
             class Payload {{ function __destruct() {{ echo 'destroy|'; }} }} \
             {call} catch (Throwable $e) {{ echo get_class($e); }}"
        );
        assert_eq!(run_php(&source), "destroy|ArgumentCountError", "{call}");
    }
}

#[test]
fn call_entry_failure_preserves_the_outer_pending_argument_owner() {
    assert_eq!(
        run_php(
            r#"<?php
ini_set('zend.exception_ignore_args', '1');
class Payload { function __destruct() { echo 'destroy|'; } }
function accept($first, $second): void {}
function onlyInt(int $value): void {}
try { accept(new Payload, onlyInt(new Payload)); }
catch (Throwable $e) { echo get_class($e); }
"#
        ),
        "destroy|destroy|TypeError"
    );
}

#[test]
fn an_exception_trace_keeps_the_argument_owner_until_the_trace_releases_it() {
    assert_eq!(
        common::run_php_with_source_context(
            r#"<?php
ini_set('zend.exception_ignore_args', '0');
class Payload { function __destruct() { echo 'destroy|'; } }
function onlyInt(int $value): void {}
try { onlyInt(new Payload); }
catch (TypeError $e) { echo 'catch|', count($e->getTrace()[0]['args']), '|'; }
unset($e);
echo 'end';
"#,
            "call-boundary.php",
            ".",
        ),
        "catch|1|destroy|end"
    );
}

#[test]
fn consumed_pending_argument_owner_survives_then_releases_on_later_argument_throw() {
    assert_eq!(
        run_php(
            r#"<?php
ini_set('zend.exception_ignore_args', '1');
class Marker { function __destruct() { echo 'destroy|'; } }
function accept($first, $second): void { echo 'body|'; }
function fail() { throw new Exception; }
try { accept(new Marker, fail()); } catch (Exception $e) { echo 'catch|'; }
echo 'end';
"#
        ),
        "destroy|catch|end"
    );
}

#[test]
fn argument_owner_return_and_referenced_rvalue_preserve_one_real_identity() {
    assert_eq!(
        run_php(
            r#"<?php
class Marker {
    public string $note = '';
    function __destruct() { echo 'destroy|'; }
}
function identity($value) { return $value; }
function &alias(&$value) { return $value; }
function update($value): void { $value->note = 'changed'; }
$marker = identity(new Marker);
echo 'held|';
update(alias($marker));
echo $marker->note, '|';
unset($marker);
echo 'end';
"#
        ),
        "held|changed|destroy|end"
    );
}

#[test]
fn global_updates_remain_visible_to_catch_and_finally_through_reference_aliases() {
    assert_eq!(
        run_php(
            r#"<?php
$value = 0;
$alias =& $value;
function fail(): void { $GLOBALS['value'] = 3; throw new Exception; }
function forward(): void { global $value; try { fail(); } finally { echo $value, '|'; } }
try { forward(); } catch (Exception $e) { echo $value, ':', $alias, '|'; }
echo $GLOBALS['value'];
"#
        ),
        "3|3:3|3"
    );
}

#[test]
fn pending_argument_destructor_exception_replaces_and_chains_the_type_error() {
    assert_eq!(
        run_php(
            r#"<?php
ini_set('zend.exception_ignore_args', '1');
class Marker {
    function __destruct() { echo 'destroy|'; throw new LogicException('release'); }
}
function onlyInt(int $value): void {}
try { onlyInt(new Marker); }
catch (Throwable $e) { echo get_class($e), ':', get_class($e->getPrevious()); }
"#
        ),
        "destroy|LogicException:TypeError"
    );
}

#[test]
fn pending_argument_destructor_exception_reselects_the_effective_catch() {
    assert_eq!(
        run_php(
            r#"<?php
ini_set('zend.exception_ignore_args', '1');
class Marker {
    function __destruct() { echo 'destroy|'; throw new LogicException('release'); }
}
function accept($a, $b): void {}
function fail() { throw new RuntimeException('argument'); }
try { accept(new Marker, fail()); }
catch (RuntimeException $e) { echo 'wrong'; }
catch (LogicException $e) { echo get_class($e), ':', get_class($e->getPrevious()); }
"#
        ),
        "destroy|LogicException:RuntimeException"
    );
}

#[test]
fn partial_wide_argument_storage_is_initialized_on_reuse_before_cleanup() {
    let parameters = (0..70)
        .map(|index| format!("$p{index}"))
        .collect::<Vec<_>>()
        .join(",");
    let arguments = ["new Marker", "fail()"]
        .into_iter()
        .chain(std::iter::repeat_n("null", 68))
        .collect::<Vec<_>>()
        .join(",");
    let source = r#"<?php
ini_set('zend.exception_ignore_args', '1');
class Marker { function __destruct() { echo 'destroy|'; } }
function accept(__PARAMETERS__): void { echo 'body|'; }
function fail() { throw new RuntimeException; }
for ($i = 0; $i < 3; $i++) {
    try { accept(__ARGUMENTS__); } catch (RuntimeException $e) { echo 'catch|'; }
}
echo 'end';
"#
    .replace("__PARAMETERS__", &parameters)
    .replace("__ARGUMENTS__", &arguments);
    assert_eq!(
        run_php(&source),
        "destroy|catch|destroy|catch|destroy|catch|end"
    );
}

#[test]
fn diagnostic_entry_failure_retires_the_argument_without_entering_the_body() {
    for attribute in ["Deprecated", "NoDiscard"] {
        let source = format!(
            "<?php ini_set('zend.exception_ignore_args', '1'); \
             class Payload {{ function __destruct() {{ echo 'destroy|'; }} }} \
             #[{attribute}] function accept($first): int {{ echo 'wrong|'; return 1; }} \
             set_error_handler(function() {{ throw new RuntimeException('diagnostic'); }}); \
             try {{ accept(new Payload); }} \
             catch (Throwable $e) {{ echo get_class($e); }}"
        );
        assert_eq!(
            common::run_php_with_source_context(&source, "call-boundary.php", "."),
            "destroy|RuntimeException",
            "{attribute}"
        );
    }
}
