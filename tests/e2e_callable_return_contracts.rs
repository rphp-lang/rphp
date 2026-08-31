mod common;

use common::{run_php, run_php_expect_error_with_source_context, run_php_with_source_context};

#[test]
fn object_and_qualified_return_contracts_survive_inheritance_and_traits() {
    assert_eq!(
        run_php(
            r#"<?php
namespace ReturnContracts;
interface Producer { public function make(): object; }
trait MakesObject { public function make(): object { return new Result(); } }
class Result {}
final class Factory implements Producer { use MakesObject; }
function qualified(): \ReturnContracts\Result { return new Result(); }
echo get_class((new Factory())->make()), "\n";
echo get_class(qualified()), "\n";
"#,
        ),
        "ReturnContracts\\Result\nReturnContracts\\Result\n"
    );
}

#[test]
fn anonymous_return_errors_use_the_parent_derived_public_name() {
    assert_eq!(
        run_php(
            r#"<?php
interface ProducesObject { public function make(): object; }
class BaseProducer implements ProducesObject { public function make(): object {} }
$producer = new class extends BaseProducer {
    public function make(): object { return 41; }
};
try { $producer->make(); }
catch (TypeError $error) { echo $error->getMessage(), "\n"; }
"#,
        ),
        "BaseProducer@anonymous::make(): Return value must be of type object, int returned\n"
    );
}

#[test]
fn bound_closure_scope_resolves_parent_for_arguments_and_returns() {
    assert_eq!(
        run_php(
            r#"<?php
class ParentValue {}
class ChildValue extends ParentValue {}
$identity = function (parent $value): parent { return $value; };
$bound = $identity->bindTo(null, ChildValue::class);
var_dump($bound(new ParentValue()) instanceof ParentValue);
"#,
        ),
        "bool(true)\n"
    );
}

#[test]
fn class_scoped_closure_traces_keep_the_bound_receiver_kind_and_object() {
    assert_eq!(
        run_php(
            r#"<?php
final class TraceFactory {
    public function make(): callable {
        return function (): void {
            $trace = debug_backtrace();
            echo $trace[0]['class'], $trace[0]['type'], get_class($trace[0]['object']), "\n";
        };
    }
}
$callable = (new TraceFactory())->make();
$callable();
"#,
        ),
        "TraceFactory->TraceFactory\n"
    );
}

#[test]
fn implicit_never_returns_name_the_callable_kind_and_are_catchable() {
    assert_eq!(
        run_php(
            r#"<?php
function stopFunction(): never { if (false) { throw new Exception(); } }
final class Stopper {
    public static function stopMethod(): never { if (false) { throw new Exception(); } }
}
foreach (['stopFunction', [Stopper::class, 'stopMethod']] as $callable) {
    try { $callable(); }
    catch (TypeError $error) { echo $error->getMessage(), "\n"; }
}
"#,
        ),
        concat!(
            "stopFunction(): never-returning function must not implicitly return\n",
            "Stopper::stopMethod(): never-returning method must not implicitly return\n",
        )
    );
}

#[test]
fn never_and_void_are_builtin_reflection_types() {
    assert_eq!(
        run_php(
            r#"<?php
function stopNow(): never { throw new Exception(); }
function finishNormally(): void {}
foreach (['stopNow', 'finishNormally'] as $function) {
    $type = (new ReflectionFunction($function))->getReturnType();
    echo $type->getName(), ':', (int) $type->isBuiltin(), ':', (int) $type->allowsNull(), "\n";
}
"#,
        ),
        "never:1:0\nvoid:1:0\n"
    );
}

#[test]
fn reflection_source_spans_include_multiline_callable_closing_braces() {
    let output = run_php_with_source_context(
        r#"<?php

class SpanProbe {
    function convert(array $value): array {
        return $value;
    }
}
echo new ReflectionClass(SpanProbe::class);
"#,
        "/virtual/return-span.php",
        "/virtual",
    );
    assert!(
        output.contains("  @@ /virtual/return-span.php 3-7\n"),
        "{output}"
    );
    assert!(
        output.contains("      @@ /virtual/return-span.php 4 - 6\n"),
        "{output}"
    );
}

#[test]
fn return_type_will_change_rejects_non_method_targets() {
    for (source, target) in [
        (
            "<?php\n#[ReturnTypeWillChange]\nclass Invalid {}\n",
            "class",
        ),
        (
            "<?php\n#[ReturnTypeWillChange]\nfunction invalid() {}\n",
            "function",
        ),
        (
            "<?php\nclass Invalid {\n#[ReturnTypeWillChange]\npublic int $value;\n}\n",
            "property",
        ),
    ] {
        let error = run_php_expect_error_with_source_context(
            source,
            "/virtual/return-attribute.php",
            "/virtual",
        );
        let rendered = error.to_string();
        assert!(
            rendered.contains(&format!(
                "Attribute \"ReturnTypeWillChange\" cannot target {target} (allowed targets: method)"
            )),
            "{rendered}"
        );
    }
}

#[test]
fn return_type_will_change_allows_methods_and_delayed_target_validation() {
    assert_eq!(
        run_php(
            r#"<?php
class ValidMethod { #[ReturnTypeWillChange] public function value() { return 1; } }
#[DelayedTargetValidation]
#[ReturnTypeWillChange]
class DeferredClass {}
echo (new ValidMethod())->value(), ':', DeferredClass::class, "\n";
"#,
        ),
        "1:DeferredClass\n"
    );

    let repeated = run_php_expect_error_with_source_context(
        "<?php\nclass Repeated { #[ReturnTypeWillChange] #[ReturnTypeWillChange] function value() {} }\n",
        "/virtual/repeated-return-attribute.php",
        "/virtual",
    );
    assert!(
        repeated
            .to_string()
            .contains("Attribute \"ReturnTypeWillChange\" must not be repeated"),
        "{repeated}"
    );
}

#[test]
fn return_expression_side_effects_commit_before_type_validation() {
    assert_eq!(
        run_php(
            r#"<?php
class WrongResult {}
function buildWrong(): array {
    echo "body\n";
    return new WrongResult();
}
try { buildWrong(); }
catch (TypeError $error) { echo $error->getMessage(), "\n"; }
echo "caught\n";
"#,
        ),
        concat!(
            "body\n",
            "buildWrong(): Return value must be of type array, WrongResult returned\n",
            "caught\n",
        )
    );
}

#[test]
fn weak_and_strict_return_coercion_remain_distinct() {
    assert_eq!(
        run_php(
            r#"<?php
function weakValue(): int { return '12'; }
var_dump(weakValue());
"#,
        ),
        "int(12)\n"
    );

    assert_eq!(
        run_php(
            r#"<?php
declare(strict_types=1);
function strictValue(): int { return '12'; }
try { strictValue(); }
catch (TypeError $error) { echo $error->getMessage(), "\n"; }
"#,
        ),
        "strictValue(): Return value must be of type int, string returned\n"
    );
}

#[test]
fn never_dead_code_may_throw_without_reaching_the_implicit_boundary() {
    assert_eq!(
        run_php(
            r#"<?php
function alwaysThrows(): never { throw new RuntimeException('done'); }
try { alwaysThrows(); }
catch (RuntimeException $error) { echo $error->getMessage(), "\n"; }
"#,
        ),
        "done\n"
    );
}
