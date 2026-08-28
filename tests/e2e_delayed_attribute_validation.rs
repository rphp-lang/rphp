mod common;

use common::{run_php, run_php_with_source_context};

#[test]
fn delayed_class_validators_repeat_without_mutating_the_declaration() {
    assert_eq!(
        run_php(
            r#"<?php
#[DelayedTargetValidation] #[AllowDynamicProperties] trait DeferredTrait {}
#[DelayedTargetValidation] #[AllowDynamicProperties] readonly class DeferredReadonly {}
#[DelayedTargetValidation] #[Attribute] interface DeferredInterface {}
#[DelayedTargetValidation] #[Attribute] abstract class DeferredAbstract {}
#[DelayedTargetValidation] #[AllowDynamicProperties] class DeferredValid {}

foreach ([DeferredTrait::class, DeferredReadonly::class, DeferredInterface::class, DeferredAbstract::class] as $class) {
    $attribute = (new ReflectionClass($class))->getAttributes()[1];
    for ($attempt = 0; $attempt < 2; ++$attempt) {
        try { $attribute->newInstance(); }
        catch (Error $error) { echo $attempt, ':', $error->getMessage(), "\n"; }
    }
}
$object = new DeferredValid();
$object->dynamic = 42;
$attribute = (new ReflectionClass($object))->getAttributes()[1];
echo get_class($attribute->newInstance()), ':', $object->dynamic;
"#,
        ),
        concat!(
            "0:Cannot apply #[\\AllowDynamicProperties] to trait DeferredTrait\n",
            "1:Cannot apply #[\\AllowDynamicProperties] to trait DeferredTrait\n",
            "0:Cannot apply #[\\AllowDynamicProperties] to readonly class DeferredReadonly\n",
            "1:Cannot apply #[\\AllowDynamicProperties] to readonly class DeferredReadonly\n",
            "0:Cannot apply #[\\Attribute] to interface DeferredInterface\n",
            "1:Cannot apply #[\\Attribute] to interface DeferredInterface\n",
            "0:Cannot apply #[\\Attribute] to abstract class DeferredAbstract\n",
            "1:Cannot apply #[\\Attribute] to abstract class DeferredAbstract\n",
            "AllowDynamicProperties:42",
        )
    );
}

#[test]
fn delayed_marker_preserves_each_public_target_and_generic_target_error() {
    assert_eq!(
        run_php(
            r#"<?php
class TargetCarrier {
    #[DelayedTargetValidation] #[Attribute] public const VALUE = 'value';
    #[DelayedTargetValidation] #[Attribute] public string $property;
    #[DelayedTargetValidation] #[Attribute]
    public function method(#[DelayedTargetValidation] #[Attribute] string $parameter): void {}
}
#[DelayedTargetValidation] #[Attribute] function targetFunction(): void {}
#[DelayedTargetValidation] #[Attribute] const TARGET_CONSTANT = 1;
$targets = [
    new ReflectionClassConstant(TargetCarrier::class, 'VALUE'),
    new ReflectionProperty(TargetCarrier::class, 'property'),
    new ReflectionMethod(TargetCarrier::class, 'method'),
    new ReflectionParameter([TargetCarrier::class, 'method'], 'parameter'),
    new ReflectionFunction('targetFunction'),
    new ReflectionConstant('TARGET_CONSTANT'),
];
foreach ($targets as $target) {
    $attribute = $target->getAttributes()[1];
    echo $attribute->getTarget(), ':';
    try { $attribute->newInstance(); }
    catch (Error $error) { echo $error->getMessage(), "\n"; }
}
"#,
        ),
        concat!(
            "16:Attribute \"Attribute\" cannot target class constant (allowed targets: class)\n",
            "8:Attribute \"Attribute\" cannot target property (allowed targets: class)\n",
            "4:Attribute \"Attribute\" cannot target method (allowed targets: class)\n",
            "32:Attribute \"Attribute\" cannot target parameter (allowed targets: class)\n",
            "2:Attribute \"Attribute\" cannot target function (allowed targets: class)\n",
            "64:Attribute \"Attribute\" cannot target constant (allowed targets: class)\n",
        )
    );
}

#[test]
fn core_delayed_validation_markers_are_real_attribute_classes() {
    assert_eq!(
        run_php(
            r#"<?php
foreach ([DelayedTargetValidation::class, AllowDynamicProperties::class, ReturnTypeWillChange::class] as $class) {
    $reflection = new ReflectionClass($class);
    $marker = $reflection->getAttributes()[0];
    echo $class, ':', $marker->getArguments()[0], ':', get_class($marker->newInstance()), '|';
}
echo get_class(new AllowDynamicProperties()), ':', get_class(new ReturnTypeWillChange());
"#,
        ),
        concat!(
            "DelayedTargetValidation:127:Attribute|",
            "AllowDynamicProperties:1:Attribute|",
            "ReturnTypeWillChange:4:Attribute|",
            "AllowDynamicProperties:ReturnTypeWillChange",
        )
    );
}

#[test]
fn tentative_countable_return_and_reflection_rendering_keep_their_boundaries() {
    assert_eq!(
        run_php_with_source_context(
            r#"<?php
class Unsuppressed implements Countable { public function count() { return 1; } }
class Suppressed implements Countable {
    #[DelayedTargetValidation] #[ReturnTypeWillChange]
    public function count() { return 2; }
}
class Rendered {
    public const VALUE = 'value';
    public string $hooked { get => $this->hooked; set => $value; }
    public function __construct(public string $promoted) {}
}
$class = (string) new ReflectionClass(Rendered::class);
echo str_contains($class, '<user> <iterateable> class Rendered'), ':';
echo str_contains($class, 'Constant [ public string VALUE ] { value }'), ':';
echo str_contains($class, '- Methods [1]'), ':';
echo trim((string) new ReflectionClassConstant(Rendered::class, 'VALUE')), ':';
echo count(new Unsuppressed()), ':', count(new Suppressed());
"#,
            "/virtual/delayed-attribute.php",
            "/virtual",
        ),
        concat!(
            "\nDeprecated: Return type of Unsuppressed::count() should either be compatible with Countable::count(): int, or the #[\\ReturnTypeWillChange] attribute should be used to temporarily suppress the notice in /virtual/delayed-attribute.php on line 2\n",
            "1:1:1:Constant [ public string VALUE ] { value }:1:2",
        )
    );
}
