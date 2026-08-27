mod common;
use common::*;

#[test]
fn ini_access_constants_cover_namespace_runtime_and_default_expression_paths() {
    assert_eq!(
        run_php(
            r#"<?php
namespace Contract\Ini;

const INI_ALL = 99;

function defaults(
    $user = INI_USER,
    $perdir = INI_PERDIR,
    $system = INI_SYSTEM,
    $all = \INI_ALL,
): void {
    echo "$user:$perdir:$system:$all|";
}

echo INI_ALL, ':', \INI_ALL, ':', INI_USER, ':', INI_PERDIR, ':', INI_SYSTEM, '|';
defaults();
$reader = constant(...);
echo $reader('INI_ALL'), ':', (new \ReflectionConstant('INI_SYSTEM'))->getValue(), '|';
var_dump(defined('INI_ALL'), defined('ini_all'));
$constants = get_defined_constants();
echo $constants['INI_USER'], ':', $constants['INI_PERDIR'], ':', $constants['INI_SYSTEM'], ':', $constants['INI_ALL'];
"#,
        ),
        "99:7:1:2:4|1:2:4:7|7:4|bool(true)\nbool(false)\n1:2:4:7"
    );
}

#[test]
fn runtime_constant_functions_validate_names_and_report_collisions() {
    assert_eq!(
        run_php_with_source_context(
            r#"<?php
set_error_handler(function($level, $message, $file, $line) { echo "$level:$message:$line\n"; });
try { define([], 1); } catch (TypeError $error) { echo $error->getMessage(), "\n"; }
var_dump(define('TRUE', 1));
var_dump(define('runtime name', 2));
var_dump(define('runtime name', 3));
try { constant('missing runtime name'); } catch (Error $error) { echo $error->getMessage(); }
"#,
            "/virtual/constants.php",
            "/virtual",
        ),
        concat!(
            "define(): Argument #1 ($constant_name) must be of type string, array given\n",
            "2:Constant TRUE already defined, this will be an error in PHP 9:4\n",
            "bool(false)\n",
            "bool(true)\n",
            "2:Constant runtime name already defined, this will be an error in PHP 9:6\n",
            "bool(false)\n",
            "Undefined constant \"missing runtime name\"",
        )
    );
}

#[test]
fn source_constant_redefinition_warns_and_preserves_the_first_value_and_attributes() {
    assert_eq!(
        run_php_with_source_context(
            r#"<?php
#[FirstMarker]
const StableValue = 1;
#[SecondMarker]
const StableValue = 2;
const stablevalue = 3;
echo StableValue, ':', stablevalue, ':';
$attributes = (new ReflectionConstant('StableValue'))->getAttributes();
echo count($attributes), ':', $attributes[0]->getName();
"#,
            "/virtual/constant-redefinition.php",
            "/virtual",
        ),
        concat!(
            "\nWarning: Constant StableValue already defined, this will be an error in PHP 9 in /virtual/constant-redefinition.php on line 5\n",
            "1:3:1:FirstMarker",
        )
    );
}

#[test]
fn source_constant_redefinition_does_not_seed_later_constants_from_the_duplicate() {
    assert_eq!(
        run_php_with_source_context(
            r#"<?php
define('RuntimeSeed', 7);
const DeferredStable = RuntimeSeed;
const DeferredStable = 9;
const DerivedStable = DeferredStable;
echo DeferredStable, ':', DerivedStable;
"#,
            "/virtual/deferred-constant-redefinition.php",
            "/virtual",
        ),
        concat!(
            "\nWarning: Constant DeferredStable already defined, this will be an error in PHP 9 in /virtual/deferred-constant-redefinition.php on line 4\n",
            "7:7",
        )
    );
}

// ============================================================================
// class-like constants
// ============================================================================

#[test]
fn test_class_constants_support_forward_references_types_and_class_name() {
    let out = run_php(
        r#"<?php
class Values {
    public const FIRST = self::SECOND;
    public const SECOND = 42, TEXT = "ok";
    public const COMPUTED = self::FIRST + 1;
    public const MESSAGE = self::TEXT . "!";
    final protected const string LABEL = "typed";

    public static function label() {
        return self::LABEL;
    }
}
echo Values::FIRST . ':' . Values::SECOND . ':' . Values::COMPUTED . ':';
echo Values::MESSAGE . ':';
echo Values::label() . ':' . Values::class . ':' . MissingClass::class;
"#,
    );
    assert_eq!(out, "42:42:43:ok!:typed:Values:MissingClass");
}

#[test]
fn constant_array_unpack_folds_forward_dependencies_with_php_key_rules() {
    assert_eq!(
        run_php(
            r#"<?php
const SEED_VALUES = [91 => 'integer', 'mode' => 'seed'];
const MERGED_VALUES = ['prefix', ...SEED_VALUES, 'mode' => 'override', ...['suffix']];

class ConstantArrays {
    public const COMBINED = ['class-prefix', ...self::SOURCE];
    public const SOURCE = [73 => 'class-integer', 'label' => 'class-string'];
    public static array $snapshot = [...self::SOURCE];
}

foreach (MERGED_VALUES as $key => $value) echo $key, '=', $value, ';';
foreach (ConstantArrays::COMBINED as $key => $value) echo $key, '=', $value, ';';
foreach (ConstantArrays::$snapshot as $key => $value) echo $key, '=', $value, ';';
"#,
        ),
        "0=prefix;1=integer;mode=override;2=suffix;0=class-prefix;1=class-integer;label=class-string;0=class-integer;label=class-string;"
    );
}

#[test]
fn self_referencing_class_constant_arrays_throw_only_when_read() {
    assert_eq!(
        run_php(
            r#"<?php
class RecursivePalette {
    public const WARM = [...self::COOL];
    public const COOL = [...self::WARM];
}

echo 'linked:';
try {
    var_dump(RecursivePalette::WARM);
} catch (Error $error) {
    echo get_class($error), ':', $error->getMessage();
}
"#,
        ),
        "linked:Error:Cannot declare self-referencing constant self::COOL"
    );
}

#[test]
fn self_referencing_class_constants_keep_declaration_and_use_trace_origins() {
    assert_eq!(
        run_php_with_source_context(
            r#"<?php
class DirectCycle {
    public const A = self::B;
    public const B = self::A;
    public static function ping(): string { return 'linked'; }
}
class ArrayCycle {
    public const A = [
        self::B,
    ];
    public const B = [
        self::A,
    ];
}
class SeedCycle { public const B = 0; }
class MixedCycle {
    public const A = SeedCycle::B
        + self::B;
    public const B = self::A;
}

function inspectCycle(string $label, Closure $operation): void {
    try {
        $operation();
    } catch (Throwable $error) {
        $trace = $error->getTrace();
        $first = $trace[0];
        $second = $trace[1] ?? [];
        $next = ($second['class'] ?? '') . ($second['type'] ?? '') . ($second['function'] ?? '');
        $next = str_starts_with($next, '{closure:') ? '{closure}' : strtolower($next);
        echo $label, ':', $error->getMessage(), '|',
            $error->getFile() === __FILE__ ? 'definition' : 'wrong-file', ':', $error->getLine(), '|',
            $first['file'] === __FILE__ ? 'use' : 'wrong-use', ':', $first['line'],
            ':', $first['function'], '|', $next, "\n";
    }
}

echo DirectCycle::ping(), "\n";
inspectCycle('direct-1', fn() => DirectCycle::A);
inspectCycle('direct-2', fn() => DirectCycle::A);
inspectCycle('array', fn() => ArrayCycle::A);
inspectCycle('mixed', fn() => MixedCycle::A);
$reflection = new ReflectionClass(DirectCycle::class);
inspectCycle('reflection', fn() => $reflection->getConstant('A'));
"#,
            "/virtual/self-referencing-constant-origin.php",
            "/virtual",
        ),
        concat!(
            "linked\n",
            "direct-1:Cannot declare self-referencing constant self::B|definition:3|use:39:[constant expression]|{closure}\n",
            "direct-2:Cannot declare self-referencing constant self::B|definition:3|use:40:[constant expression]|{closure}\n",
            "array:Cannot declare self-referencing constant self::B|definition:9|use:41:[constant expression]|{closure}\n",
            "mixed:Cannot declare self-referencing constant self::B|definition:18|use:42:[constant expression]|{closure}\n",
            "reflection:Cannot declare self-referencing constant self::B|definition:3|use:44:[constant expression]|reflectionclass->getconstant\n",
        )
    );
}

#[test]
fn deferred_constant_expression_unpack_does_not_accept_traversable_objects() {
    assert_eq!(
        run_php(
            r#"<?php
function defaultPalette($values = [...new ArrayObject(['default'])]) {
    return $values;
}
function staticPalette() {
    static $values = [...new ArrayObject(['static'])];
    return $values;
}

try { defaultPalette(); } catch (Error $error) { echo 'default:', $error->getMessage(), ';'; }
try { staticPalette(); } catch (Error $error) { echo 'static:', $error->getMessage(); }
"#,
        ),
        "default:Only arrays can be unpacked in constant expression;static:Only arrays can be unpacked in constant expression"
    );
}

#[test]
fn constant_expression_property_fetches_keep_enum_nullsafe_and_runtime_paths() {
    assert_eq!(
        run_php(
            r#"<?php
enum UnitSignal { case Ready; }
enum BackedSignal: string { case Ready = 'ready'; }

const UNIT_NAME = UnitSignal::Ready?->name;
const BACKED_VALUE = BackedSignal::Ready->value;
const NULLSAFE_VALUE = null?->missing;

function enumDefaults(
    $name = UnitSignal::Ready->name,
    $value = BackedSignal::Ready?->value,
    $empty = null?->missing,
): void {
    var_dump($name, $value, $empty);
}

enumDefaults();
class RuntimeProperty { public $value = 'runtime'; }
$object = new RuntimeProperty();
echo UNIT_NAME, ':', BACKED_VALUE, ':', $object->value, ':', $object?->value;
"#,
        ),
        concat!(
            "string(5) \"Ready\"\n",
            "string(5) \"ready\"\n",
            "NULL\n",
            "Ready:ready:runtime:runtime",
        )
    );
}

#[test]
fn non_enum_constant_property_errors_follow_name_order_and_repeat_for_defaults() {
    assert_eq!(
        run_php(
            r#"<?php
class ConstantOwner {
    public $chosen = 42;
    public function __construct() { echo 'owner|'; }
}
class DynamicPropertyName {
    public function __construct() { echo 'name|'; }
    public function __toString() { echo 'string|'; return 'chosen'; }
}
function materialize($value = (new ConstantOwner)->{new DynamicPropertyName}) {
    echo "body:$value|";
}

materialize('given');
for ($attempt = 0; $attempt < 2; $attempt++) {
    try {
        materialize();
    } catch (Error $error) {
        echo get_class($error), ':', $error->getMessage(), '|';
    }
}
"#,
        ),
        concat!(
            "body:given|",
            "owner|name|string|Error:Fetching properties on non-enums in constant expressions is not allowed|",
            "owner|name|string|Error:Fetching properties on non-enums in constant expressions is not allowed|",
        )
    );
}

#[test]
fn non_enum_nullsafe_property_of_another_constant_stops_at_the_fetch_origin() {
    let error = run_php_expect_error_with_source_context(
        r#"<?php
class DeferredObject { public $value = 7; }
const OWNER = new DeferredObject();
const INVALID =
    OWNER?->value;
echo 'unreachable';
"#,
        "/virtual/non-enum-constant-property.php",
        "/virtual",
    );

    let rphp::vm::execute::VmError::Fatal(message) = error else {
        panic!("expected a fatal error");
    };
    assert_eq!(
        message,
        concat!(
            "Uncaught Error: Fetching properties on non-enums in constant expressions is not allowed ",
            "in /virtual/non-enum-constant-property.php:5\n",
            "Stack trace:\n",
            "#0 {main}\n",
            "  thrown in /virtual/non-enum-constant-property.php on line 5",
        )
    );
}

#[test]
fn enum_property_values_materialize_across_declaration_initializers() {
    assert_eq!(
        run_php(
            r#"<?php
enum SourcePulse: int { case Live = 73; }
enum CopiedName: string { case Live = SourcePulse::Live->name; }
enum CopiedValue: int { case Live = SourcePulse::Live?->value; }

class InitializerSnapshot {
    public const NAME = SourcePulse::Live?->name;
    public const VALUE = SourcePulse::Live->value;
    public string $name = SourcePulse::Live->name;
    public int $value = SourcePulse::Live?->value;
    public static string $staticName = SourcePulse::Live?->name;
    public static int $staticValue = SourcePulse::Live->value;
}

echo CopiedName::Live->value, ':', CopiedValue::Live->value, '|';
for ($attempt = 0; $attempt < 2; $attempt++) {
    $snapshot = new InitializerSnapshot();
    echo InitializerSnapshot::NAME, ':', InitializerSnapshot::VALUE, ':';
    echo $snapshot->name, ':', $snapshot->value, '|';
}
echo InitializerSnapshot::$staticName, ':', InitializerSnapshot::$staticValue;
"#,
        ),
        "Live:73|Live:73:Live:73|Live:73:Live:73|Live:73"
    );
}

#[test]
fn enum_property_initializer_folding_keeps_alias_nested_and_dynamic_forms() {
    assert_eq!(
        run_php(
            r#"<?php
namespace SourceDomain {
    enum Status: string { case Ready = 'ready'; }
}
namespace ProjectionDomain {
    use SourceDomain\Status as ImportedStatus;

    class Projection {
        public const VALUES = [
            'name' => ImportedStatus::Ready->name,
            'value' => ImportedStatus::Ready?->value,
            'dynamic' => ImportedStatus::Ready->{'name'},
        ];
        public array $values = [
            ImportedStatus::Ready?->name,
            ImportedStatus::Ready->value,
        ];
        public static array $staticValues = [
            ImportedStatus::Ready->{'name'},
            ImportedStatus::Ready?->value,
        ];
    }

    $projection = new Projection();
    echo implode('|', Projection::VALUES), "\n";
    echo implode('|', $projection->values), "\n";
    echo implode('|', Projection::$staticValues);
}
"#,
        ),
        "Ready|ready|Ready\nReady|ready\nReady|ready"
    );
}

#[test]
fn enum_property_instance_initializer_can_wait_for_a_later_enum_declaration() {
    assert_eq!(
        run_php(
            r#"<?php
class DeferredSnapshot {
    public const NAME = FuturePulse::Ready?->name;
    public string $name = FuturePulse::Ready?->name;
    public string $value = FuturePulse::Ready->value;
}

enum FuturePulse: string { case Ready = 'future'; }

for ($attempt = 0; $attempt < 2; $attempt++) {
    $snapshot = new DeferredSnapshot();
    echo DeferredSnapshot::NAME, ':', $snapshot->name, ':', $snapshot->value, '|';
}
"#,
        ),
        "Ready:Ready:future|Ready:Ready:future|"
    );
}

#[test]
fn deferred_declaration_property_fetch_keeps_the_non_enum_error() {
    assert_eq!(
        run_php(
            r#"<?php
define('LATE_NON_ENUM', new stdClass());
class DeferredNonEnum {
    public const BAD = LATE_NON_ENUM->missing;
}

try {
    var_dump(DeferredNonEnum::BAD);
} catch (Error $error) {
    echo get_class($error), ':', $error->getMessage();
}
"#,
        ),
        "Error:Fetching properties on non-enums in constant expressions is not allowed"
    );
}

#[test]
fn goto_keyword_spelling_is_preserved_for_class_constant_names() {
    assert_eq!(
        run_php(
            r#"<?php
class Labels {
    public const GOTO = "upper";
    public const goto = "lower";
}
echo Labels::GOTO, ":", Labels::goto;
"#,
        ),
        "upper:lower"
    );
}

#[test]
fn class_name_literals_resolve_inside_nested_property_defaults() {
    let out = run_php(
        r#"<?php
namespace App;
use Vendor\Attribute as Alias;
class Defaults {
    public array $values = [Alias::class => Alias::class, Local::class => Local::class];
}
$values = (new Defaults())->values;
echo $values[Alias::class] . ':' . $values[Local::class];
"#,
    );
    assert_eq!(out, "Vendor\\Attribute:App\\Local");
}

#[test]
fn deferred_instance_defaults_resolve_parent_first_and_initialize_each_object() {
    assert_eq!(
        run_php(
            r#"<?php
spl_autoload_register(function ($class) {
    echo $class, '>';
    eval("class $class { const VALUE = '$class'; }");
});

class DeferredParent {
    public $parent = ParentSymbol::VALUE;
}
class DeferredChild extends DeferredParent {
    public $child = ChildSymbol::VALUE;
}

$first = new DeferredChild();
$second = new DeferredChild();
echo $first->parent, ':', $first->child, ':', $second->parent, ':', $second->child;
"#,
        ),
        "ParentSymbol>ChildSymbol>ParentSymbol:ChildSymbol:ParentSymbol:ChildSymbol"
    );
}

#[test]
fn deferred_typed_property_default_failures_are_retryable() {
    assert_eq!(
        run_php(
            r#"<?php
define('DYNAMIC_DEFAULT', 5);
class GoodDeferredDefault { public int $value = DYNAMIC_DEFAULT; }
class BadDeferredDefault { public string $value = DYNAMIC_DEFAULT; }

echo (new GoodDeferredDefault())->value, ':';
for ($attempt = 0; $attempt < 2; $attempt++) {
    try {
        new BadDeferredDefault();
    } catch (TypeError $error) {
        echo $error->getMessage(), ';';
    }
}
"#,
        ),
        concat!(
            "5:",
            "Cannot assign int to property BadDeferredDefault::$value of type string;",
            "Cannot assign int to property BadDeferredDefault::$value of type string;",
        )
    );
}

#[test]
fn deferred_trait_defaults_bind_to_the_consumer_and_shadowed_defaults_do_not_run() {
    assert_eq!(
        run_php(
            r#"<?php
define('TRAIT_DEFAULT', 7);
trait FirstDefault { public $shared = TRAIT_DEFAULT; }
trait EqualDefault { public $shared = TRAIT_DEFAULT; }
trait RelativeDefault { public $relative = self::VALUE; }

class DeferredConsumer {
    use FirstDefault, EqualDefault, RelativeDefault;
    const VALUE = 8;
}
class DeferredBase { public $discarded = NEVER_DEFINED; }
class ShadowingChild extends DeferredBase { public $discarded = 42; }

$consumer = new DeferredConsumer();
echo $consumer->shared, ':', $consumer->relative, ':', (new ShadowingChild())->discarded;
"#,
        ),
        "7:8:42"
    );
}

#[test]
fn invalid_property_expressions_are_not_postponed_by_a_symbol_reference() {
    let error = run_php_expect_error(
        "<?php class InvalidDeferredDefault { public $value = MISSING + strlen('x'); }",
    );
    assert!(
        error
            .to_string()
            .contains("Cannot use non-constant expression as default value for property InvalidDeferredDefault::$value")
    );
}

#[test]
fn php_url_component_constants_are_available() {
    assert_eq!(
        run_php(
            "<?php echo PHP_URL_SCHEME . PHP_URL_HOST . PHP_URL_PORT . PHP_URL_USER . PHP_URL_PASS . PHP_URL_PATH . PHP_URL_QUERY . PHP_URL_FRAGMENT;"
        ),
        "01234567"
    );
}

#[test]
fn test_class_constants_are_composed_and_late_static_reads_are_cached_by_class() {
    let out = run_php(
        r#"<?php
interface Numbered { public const INTERFACE_VALUE = 7; }
trait Tagged {
    protected const TRAIT_VALUE = 8;
    private const PRIVATE_VALUE = 9;
    public static function privateValue() { return self::PRIVATE_VALUE; }
}
class ConstantBase {
    protected const VALUE = 10;
    public static function value() { return static::VALUE; }
}
class ConstantChild extends ConstantBase implements Numbered {
    use Tagged;
    public const VALUE = 11;
    public static function combined() {
        return self::INTERFACE_VALUE + self::TRAIT_VALUE;
    }
}
echo ConstantBase::value() . ':' . ConstantChild::value() . ':';
echo ConstantChild::value() . ':' . ConstantChild::combined() . ':';
echo ConstantChild::privateValue() . ':' . ConstantChild::INTERFACE_VALUE;
"#,
    );
    assert_eq!(out, "10:11:11:15:9:7");
}

#[test]
fn trait_constant_composition_preserves_identity_origin_and_reflection_values() {
    assert_eq!(
        run_php(
            r#"<?php
#[Attribute(Attribute::TARGET_CLASS_CONSTANT)]
class ConstantMarker {}
trait LeftConstants {
    #[ConstantMarker]
    public const array VALUE = [1];
}
trait RightConstants { public const array VALUE = [1]; }
class ConstantConsumer { use LeftConstants, RightConstants; }

$trait = new ReflectionClass(LeftConstants::class);
$consumer = new ReflectionClass(ConstantConsumer::class);
echo $trait->getConstant('VALUE')[0], ':';
echo $trait->getReflectionConstant('VALUE')->getDeclaringClass()->getName(), ':';
echo $consumer->getConstant('VALUE')[0], ':';
$constant = $consumer->getReflectionConstant('VALUE');
echo $constant->getDeclaringClass()->getName(), ':';
echo count($constant->getAttributes(ConstantMarker::class));
"#,
        ),
        "1:LeftConstants:1:ConstantConsumer:1"
    );
}

#[test]
fn trait_constant_composition_reports_exact_origins_and_final_parent_conflicts() {
    let cases = [
        (
            "<?php\ntrait PublicConstant { public const VALUE = 42; }\nclass PrivateConsumer { use PublicConstant; private const VALUE = 42; }",
            "PrivateConsumer and PublicConstant define the same constant (VALUE) in the composition of PrivateConsumer. However, the definition differs and is considered incompatible. Class was composed in /virtual/trait-constants.php on line 3",
        ),
        (
            "<?php\ntrait FirstConstant { public const VALUE = 42; }\ntrait SecondConstant { private const VALUE = 42; }\nclass PairConsumer { use FirstConstant, SecondConstant; }",
            "FirstConstant and SecondConstant define the same constant (VALUE) in the composition of PairConsumer. However, the definition differs and is considered incompatible. Class was composed in /virtual/trait-constants.php on line 4",
        ),
        (
            "<?php\ntrait FinalProvider { public final const VALUE = 42; }\nclass FinalParent { public final const VALUE = 42; }\nclass FinalChild extends FinalParent { use FinalProvider; }",
            "FinalChild::VALUE cannot override final constant FinalParent::VALUE in /virtual/trait-constants.php on line 4",
        ),
    ];
    for (source, expected) in cases {
        let error = run_php_expect_error_with_source_context(
            source,
            "/virtual/trait-constants.php",
            "/virtual",
        );
        assert_eq!(format!("{error:?}"), format!("Fatal(\"{expected}\")"));
    }
}

#[test]
fn direct_trait_constant_errors_retain_the_fetch_source_location() {
    assert_eq!(
        run_php_with_source_context(
            "<?php\ntrait DirectConstant { public const VALUE = 1; }\ntry { echo DirectConstant::VALUE; } catch (Error $error) { echo $error->getFile(), ':', $error->getLine(), ':', count($error->getTrace()); }",
            "/virtual/trait-constants.php",
            "/virtual",
        ),
        "/virtual/trait-constants.php:3:0"
    );
}

#[test]
fn trait_consumers_and_their_descendants_publish_at_their_source_declarations() {
    assert_eq!(
        run_php(
            r#"<?php
trait RuntimeMarker {}
echo (int) class_exists('RuntimeConsumer', false), ':';
echo (int) class_exists('RuntimeChild', false), ';';
class RuntimeConsumer { use RuntimeMarker; }
class RuntimeChild extends RuntimeConsumer {}
echo (int) class_exists('RuntimeConsumer', false), ':';
echo (int) class_exists('RuntimeChild', false);
"#,
        ),
        "0:0;1:1"
    );
}

#[test]
fn runtime_trait_composition_autoloads_dependencies_after_prior_output() {
    assert_eq!(
        run_php(
            r#"<?php
spl_autoload_register(function($symbol) {
    if ($symbol === 'RuntimeLoadedTrait') {
        eval("trait RuntimeLoadedTrait { public const VALUE = 'loaded'; }");
    }
});
echo 'before:';
class RuntimeAutoloadConsumer { use RuntimeLoadedTrait; }
echo RuntimeAutoloadConsumer::VALUE;
"#,
        ),
        "before:loaded"
    );
}

#[test]
fn caught_missing_trait_allows_the_runtime_declaration_to_be_retried() {
    assert_eq!(
        run_php(
            r#"<?php
function publishConsumer() {
    class RetryConsumer { use RuntimeMissingTrait; }
}
try {
    publishConsumer();
} catch (Error $error) {
    echo $error->getMessage(), ';';
}
eval("trait RuntimeMissingTrait { public const VALUE = 7; }");
publishConsumer();
echo RetryConsumer::VALUE;
"#,
        ),
        "Trait \"RuntimeMissingTrait\" not found;7"
    );
}

#[test]
fn test_class_constant_visibility_errors_are_catchable() {
    let out = run_php(
        r#"<?php
class SecretConstants {
    private const PRIVATE_VALUE = 1;
    protected const PROTECTED_VALUE = 2;
}
try { echo SecretConstants::PRIVATE_VALUE; } catch (Error $error) { echo "private"; }
echo ':';
try { echo SecretConstants::PROTECTED_VALUE; } catch (Error $error) { echo "protected"; }
"#,
    );
    assert_eq!(out, "private:protected");
}

#[test]
fn test_final_and_typed_class_constant_contracts_are_validated() {
    let final_error = run_php_expect_error(
        r#"<?php
class FinalConstantBase { final public const VALUE = 1; }
class FinalConstantChild extends FinalConstantBase { public const VALUE = 2; }
"#,
    );
    assert!(format!("{final_error:?}").contains("cannot override final constant"));

    let type_error = run_php_expect_error(
        r#"<?php
class TypedConstant { public const int VALUE = "wrong"; }
"#,
    );
    assert!(format!("{type_error:?}").contains("for class constant TypedConstant::VALUE"));
}

#[test]
fn typed_class_constant_declaration_diagnostics_match_php() {
    for (source, expected) in [
        (
            "<?php\nclass EagerTyped {\n    public const int VALUE = 'wrong';\n}",
            "Cannot use string as value for class constant EagerTyped::VALUE of type int in /virtual/typed-class-constants.php on line 3",
        ),
        (
            "<?php\nclass ForbiddenTyped {\n    public const callable VALUE = null;\n}",
            "Class constant ForbiddenTyped::VALUE cannot have type callable in /virtual/typed-class-constants.php on line 3",
        ),
    ] {
        let error = run_php_expect_error_with_source_context(
            source,
            "/virtual/typed-class-constants.php",
            "/virtual",
        );
        assert_eq!(format!("{error:?}"), format!("Fatal(\"{expected}\")"));
    }
}

#[test]
fn deferred_typed_class_constants_validate_the_origin_and_static_type() {
    assert_eq!(
        run_php(
            r#"<?php
class DeferredFloat { public const float VALUE = FLOAT_SEED; }
class DeferredBad {
    public const stdClass&Stringable ORIGIN = BAD_SEED;
    public const stdClass&Stringable DEPENDENT = self::ORIGIN;
}
enum StaticOwner { public const static VALUE = OtherEnum::CaseValue; }
enum OtherEnum { case CaseValue; }

define('FLOAT_SEED', 3);
define('BAD_SEED', new stdClass());
var_dump(DeferredFloat::VALUE);
foreach (['DEPENDENT', 'ORIGIN', 'DEPENDENT'] as $name) {
    try { var_dump(DeferredBad::{$name}); }
    catch (TypeError $error) { echo $error->getMessage(), "\n"; }
}
try { var_dump(StaticOwner::VALUE); }
catch (TypeError $error) { echo $error->getMessage(); }
"#,
        ),
        concat!(
            "float(3)\n",
            "Cannot assign stdClass to class constant DeferredBad::ORIGIN of type stdClass&Stringable\n",
            "Cannot assign stdClass to class constant DeferredBad::ORIGIN of type stdClass&Stringable\n",
            "Cannot assign stdClass to class constant DeferredBad::ORIGIN of type stdClass&Stringable\n",
            "Cannot assign OtherEnum to class constant StaticOwner::VALUE of type static",
        )
    );
}

#[test]
fn object_activation_materializes_deferred_class_constants_before_allocation() {
    assert_eq!(
        run_php(
            r#"<?php
class DiamondActivation { public const self VALUE = DIAMOND_VALUE; }
try { define('DIAMOND_VALUE', new DiamondActivation()); }
catch (Error $error) { echo 'diamond:', $error->getMessage(); }
echo defined('DIAMOND_VALUE') ? ':defined|' : ':missing|';

class RetryActivation { public const VALUE = LATE_VALUE; }
try { new RetryActivation(); }
catch (Error $error) { echo 'retry:', $error->getMessage(), ':'; }
define('LATE_VALUE', 7);
echo get_class(new RetryActivation()), ':', RetryActivation::VALUE, '|';

class InvalidActivation { public const int VALUE = INVALID_VALUE; }
define('INVALID_VALUE', 'bad');
for ($attempt = 0; $attempt < 2; $attempt++) {
    try { new InvalidActivation(); }
    catch (TypeError $error) { echo 'typed:', $error->getMessage(), ';'; }
}

class ReflectedActivation { public const int VALUE = REFLECTED_VALUE; }
$reflection = new ReflectionClass(ReflectedActivation::class);
try { $reflection->newInstanceWithoutConstructor(); }
catch (Error $error) { echo 'reflection:', $error->getMessage(), ':'; }
define('REFLECTED_VALUE', 9);
echo get_class($reflection->newInstanceWithoutConstructor());
"#,
        ),
        concat!(
            "diamond:Undefined constant \"DIAMOND_VALUE\":missing|",
            "retry:Undefined constant \"LATE_VALUE\":RetryActivation:7|",
            "typed:Cannot assign string to class constant InvalidActivation::VALUE of type int;",
            "typed:Cannot assign string to class constant InvalidActivation::VALUE of type int;",
            "reflection:Undefined constant \"REFLECTED_VALUE\":ReflectedActivation",
        )
    );
}

#[test]
fn deferred_class_constant_errors_keep_the_expression_origin_and_use_trace() {
    assert_eq!(
        run_php_with_source_context(
            r#"<?php
class DeferredOrigin {
    public const VALUE =
        self::BASE
        + MissingDeferredOrigin::VALUE;
    public const BASE = 42;
}

function inspectDeferred(string $label, Closure $operation): void {
    try {
        $operation();
    } catch (Throwable $error) {
        $trace = $error->getTrace();
        $first = $trace[0];
        $second = $trace[1] ?? [];
        $next = ($second['class'] ?? '') . ($second['type'] ?? '') . ($second['function'] ?? '');
        $next = str_starts_with($next, '{closure:') ? '{closure}' : strtolower($next);
        echo $label, ':', $error->getFile() === __FILE__ ? 'definition' : 'wrong-file',
            ':', $error->getLine(), '|',
            $first['file'] === __FILE__ ? 'use' : 'wrong-use', ':', $first['line'],
            ':', $first['function'], '|',
            $next, "\n";
    }
}

inspectDeferred('new', fn() => new DeferredOrigin());
inspectDeferred('read', fn() => DeferredOrigin::VALUE);
inspectDeferred('constant', fn() => constant('DeferredOrigin::VALUE'));
$class = new ReflectionClass(DeferredOrigin::class);
inspectDeferred('reflection-constant', fn() => $class->getConstant('VALUE'));
inspectDeferred('reflection-new', fn() => $class->newInstanceWithoutConstructor());
"#,
            "/virtual/deferred-constant-origin.php",
            "/virtual",
        ),
        concat!(
            "new:definition:5|use:26:[constant expression]|{closure}\n",
            "read:definition:5|use:27:[constant expression]|{closure}\n",
            "constant:definition:5|use:28:[constant expression]|constant\n",
            "reflection-constant:definition:5|use:30:[constant expression]|reflectionclass->getconstant\n",
            "reflection-new:definition:5|use:31:[constant expression]|reflectionclass->newinstancewithoutconstructor\n",
        )
    );
}

#[test]
fn deferred_class_constant_activation_is_inherited_but_not_eager() {
    assert_eq!(
        run_php(
            r#"<?php
class PassiveActivation {
    public const VALUE = PASSIVE_VALUE;
    public static function ping() { return 'p'; }
}
echo class_exists(PassiveActivation::class) ? 'exists:' : 'missing:';
echo PassiveActivation::ping(), ':';
define('PASSIVE_VALUE', 3);
echo (new PassiveActivation())::VALUE, '|';

class ParentActivation { public const VALUE = INHERITED_VALUE; }
class ChildActivation extends ParentActivation {}
try { new ChildActivation(); }
catch (Error $error) { echo $error->getMessage(), ':'; }
define('INHERITED_VALUE', 4);
echo get_class(new ChildActivation()), ':', ChildActivation::VALUE, '|';

abstract class AbstractActivation { public const VALUE = ABSTRACT_VALUE; }
try { new AbstractActivation(); }
catch (Error $error) { echo $error->getMessage(); }
"#,
        ),
        concat!(
            "exists:p:3|",
            "Undefined constant \"INHERITED_VALUE\":ChildActivation:4|",
            "Cannot instantiate abstract class AbstractActivation",
        )
    );
}

#[test]
fn typed_class_constant_covariance_accepts_subclasses_and_dnf_types() {
    assert_eq!(
        run_php(
            r#"<?php
class ConstantSuper implements Stringable {
    public function __toString() { return ''; }
}
class ConstantSub extends ConstantSuper {}
class ConstantOwner {}
class CovariantConstants extends ConstantOwner {
    public const object OBJECT_VALUE = SUPER_VALUE;
    public const ConstantSuper CLASS_VALUE = SUPER_VALUE;
    public const ?ConstantSuper NULLABLE_VALUE = SUPER_VALUE;
    public const ConstantOwner OWNER_VALUE = OWNER_VALUE;
}
class NarrowConstants extends CovariantConstants {
    public const ConstantSuper OBJECT_VALUE = SUB_VALUE;
    public const ConstantSub CLASS_VALUE = SUB_VALUE;
    public const (ConstantSuper&Stringable)|null NULLABLE_VALUE = SUB_VALUE;
    public const CovariantConstants OWNER_VALUE = NARROW_VALUE;
}
define('SUPER_VALUE', new ConstantSuper());
define('SUB_VALUE', new ConstantSub());
define('OWNER_VALUE', new ConstantOwner());
define('NARROW_VALUE', new CovariantConstants());
echo get_class(NarrowConstants::OBJECT_VALUE), ':';
echo get_class(NarrowConstants::CLASS_VALUE), ':';
echo get_class(NarrowConstants::NULLABLE_VALUE), ':';
echo get_class(NarrowConstants::OWNER_VALUE);
"#,
        ),
        "ConstantSub:ConstantSub:ConstantSub:CovariantConstants"
    );
}

#[test]
fn test_dynamic_class_constant_owners_and_names_rekey_one_cache_site() {
    let out = run_php(
        r#"<?php
class DynamicA {
    public const FIRST = "a1";
    public const SECOND = "a2";
}
class DynamicB {
    public const FIRST = "b1";
    public const SECOND = "b2";
}
function fixedConstant($owner) { return $owner::FIRST; }
function namedConstant($owner, $name) { return $owner::{$name}; }

echo fixedConstant(DynamicA::class) . ':';
echo fixedConstant(DynamicB::class) . ':';
echo fixedConstant(new DynamicA()) . ':';
echo fixedConstant(new DynamicB()) . ':';
echo namedConstant(DynamicA::class, 'FIRST') . ':';
echo namedConstant(DynamicA::class, 'SECOND') . ':';
echo namedConstant(DynamicB::class, 'FIRST') . ':';
echo namedConstant(new DynamicB(), 'SECOND');
"#,
    );
    assert_eq!(out, "a1:b1:a1:b1:a1:a2:b1:b2");
}

#[test]
fn test_dynamic_class_constants_preserve_evaluation_late_static_and_visibility() {
    let out = run_php(
        r#"<?php
class DynamicBase {
    protected const SECRET = "base-secret";
    public const VALUE = "base";
    public static function late($name) { return static::{$name}; }
    public static function lexical($name) { return self::{$name}; }
    public function own($name) { return $this::{$name}; }
}
class DynamicChild extends DynamicBase {
    public const VALUE = "child";
}
function ownerExpression() { echo 'O'; return DynamicChild::class; }
function nameExpression() { echo 'N'; return 'VALUE'; }

echo DynamicBase::late('VALUE') . ':' . DynamicChild::late('VALUE') . ':';
echo DynamicChild::lexical('VALUE') . ':';
echo (new DynamicBase())->own('SECRET') . ':';
echo ownerExpression()::{nameExpression()};
try {
    $name = 'SECRET';
    echo DynamicBase::{$name};
} catch (Error $error) {
    echo ':' . $error->getMessage();
}
"#,
    );
    assert_eq!(
        out,
        "base:child:base:base-secret:ONchild:Cannot access protected constant DynamicBase::SECRET"
    );
}

#[test]
fn test_dynamic_class_keyword_distinguishes_runtime_and_compile_time_names() {
    let out = run_php(
        r#"<?php
class DynamicClassName { public const CLASS_NAME = 'class'; }
const DYNAMIC_CLASS_KEYWORD = 'class';
$object = new DynamicClassName();
$owner = DynamicClassName::class;
$name = 'class';

echo $object::class . ':';
echo $owner::{$name} . ':';
echo DynamicClassName::{$name} . ':';
echo DynamicClassName::{DYNAMIC_CLASS_KEYWORD} . ':';
try { echo $owner::class; } catch (TypeError $error) { echo $error->getMessage(); }
echo ':';
try { echo DynamicClassName::{"class"}; } catch (Error $error) { echo $error->getMessage(); }
echo ':';
try { echo $owner::{"cl" . "ass"}; } catch (Error $error) { echo $error->getMessage(); }
echo ':';
try { echo DynamicClassName::{DynamicClassName::CLASS_NAME}; }
catch (Error $error) { echo $error->getMessage(); }
echo ':';
try { echo DynamicClassName::{true ? 'class' : 'missing'}; }
catch (Error $error) { echo $error->getMessage(); }
"#,
    );
    assert_eq!(
        out,
        "DynamicClassName:DynamicClassName:DynamicClassName:DynamicClassName:Cannot use \"::class\" on string:Undefined constant DynamicClassName::class:Undefined constant DynamicClassName::class:Undefined constant DynamicClassName::class:Undefined constant DynamicClassName::class"
    );
}

#[test]
fn test_dynamic_class_constant_type_errors_follow_php_resolution_order() {
    let out = run_php(
        r#"<?php
class DynamicTypeOwner { public const VALUE = 1; }
function dynamicTypeFetch($owner, $name) { return $owner::{$name}; }

try { dynamicTypeFetch(42, 42); }
catch (Error $error) { echo $error->getMessage(); }
echo ':';
try { dynamicTypeFetch('MissingDynamicOwner', 42); }
catch (Error $error) { echo $error->getMessage(); }
echo ':';
try { dynamicTypeFetch(DynamicTypeOwner::class, 42); }
catch (TypeError $error) { echo $error->getMessage(); }
"#,
    );
    assert_eq!(
        out,
        "Class name must be a valid object or a string:Class \"MissingDynamicOwner\" not found:Cannot use value of type int as class constant name"
    );
}

#[test]
fn test_dynamic_enum_cases_and_constants_do_not_alias_cache_entries() {
    let out = run_php(
        r#"<?php
enum DynamicSuit {
    case Hearts;
    case Spades;
    public const LABEL = 'suit';
}
function dynamicEnumMember($name) { return DynamicSuit::{$name}; }

echo dynamicEnumMember('LABEL') . ':';
echo (dynamicEnumMember('Hearts') === DynamicSuit::Hearts ? 'heart' : 'bad') . ':';
echo dynamicEnumMember('LABEL') . ':';
echo (dynamicEnumMember('Spades') === DynamicSuit::Spades ? 'spade' : 'bad');
"#,
    );
    assert_eq!(out, "suit:heart:suit:spade");
}

#[test]
fn test_dynamic_class_constant_arrow_capture_and_generator_suspension() {
    let out = run_php(
        r#"<?php
class DynamicSuspended { public const VALUE = 'resolved'; }
$owner = DynamicSuspended::class;
$name = 'VALUE';
$fetch = fn() => $owner::{$name};
echo $fetch() . ':';

function suspendedDynamicConstant() {
    return (yield 'owner')::{yield 'name'};
}
$generator = suspendedDynamicConstant();
echo $generator->current() . ':';
echo $generator->send(DynamicSuspended::class) . ':';
$generator->send('VALUE');
echo $generator->getReturn();
"#,
    );
    assert_eq!(out, "resolved:owner:name:resolved");
}

#[test]
fn test_dynamic_class_constant_fetches_are_valid_constant_expressions() {
    let out = run_php(
        r#"<?php
class DynamicConstantExpression {
    public const BA = 'BA';
    public const R = 'R';
    public const BAR = 'bar';
    public const DynamicConstantExpression = 'bar';
    public const FIRST = self::{'BAR'};
    public const SECOND = self::{'BA' . 'R'};
    public const THIRD = self::{self::BA . self::R};
}
const DYNAMIC_CONST_EXPRESSION = DynamicConstantExpression::{DynamicConstantExpression::class};
echo DynamicConstantExpression::FIRST . ':';
echo DynamicConstantExpression::SECOND . ':';
echo DynamicConstantExpression::THIRD . ':';
echo DYNAMIC_CONST_EXPRESSION;
"#,
    );
    assert_eq!(out, "bar:bar:bar:bar");
}

// ============================================================================
// const keyword
// ============================================================================

#[test]
fn test_const_basic() {
    let out = run_php(
        r#"<?php
const FOO = 42;
echo FOO;
"#,
    );
    assert_eq!(out, "42");
}

#[test]
fn test_const_string() {
    let out = run_php(
        r#"<?php
const GREETING = "hello";
echo GREETING;
"#,
    );
    assert_eq!(out, "hello");
}

#[test]
fn test_const_in_expression() {
    let out = run_php(
        r#"<?php
const X = 10;
const Y = 20;
echo X + Y;
"#,
    );
    assert_eq!(out, "30");
}

#[test]
fn test_const_bool_and_null() {
    let out = run_php(
        r#"<?php
const A = true;
const B = false;
const C = null;
echo A;
echo B;
echo C;
"#,
    );
    assert_eq!(out, "1");
}

#[test]
fn test_const_float() {
    let out = run_php(
        r#"<?php
const PI = 3.14;
echo PI;
"#,
    );
    assert_eq!(out, "3.14");
}

#[test]
fn test_const_used_in_function() {
    let out = run_php(
        r#"<?php
const MAX = 100;
function getMax() {
    return MAX;
}
echo getMax();
"#,
    );
    assert_eq!(out, "100");
}

#[test]
fn test_const_in_condition() {
    let out = run_php(
        r#"<?php
const DEBUG = true;
if (DEBUG) {
    echo "debug on";
} else {
    echo "debug off";
}
"#,
    );
    assert_eq!(out, "debug on");
}

// ============================================================================
// define() function
// ============================================================================

#[test]
fn test_define_basic() {
    let out = run_php(
        r#"<?php
define("BAR", 99);
echo BAR;
"#,
    );
    assert_eq!(out, "99");
}

#[test]
fn test_define_string_value() {
    let out = run_php(
        r#"<?php
define("APP_NAME", "MyApp");
echo APP_NAME;
"#,
    );
    assert_eq!(out, "MyApp");
}

#[test]
fn test_defined_true() {
    let out = run_php(
        r#"<?php
const THING = 1;
if (defined("THING")) {
    echo "yes";
} else {
    echo "no";
}
"#,
    );
    assert_eq!(out, "yes");
}

#[test]
fn test_defined_false() {
    let out = run_php(
        r#"<?php
if (defined("NOPE")) {
    echo "yes";
} else {
    echo "no";
}
"#,
    );
    assert_eq!(out, "no");
}

#[test]
fn test_constant_function() {
    let out = run_php(
        r#"<?php
define("KEY", "value123");
echo constant("KEY");
"#,
    );
    assert_eq!(out, "value123");
}

#[test]
fn test_define_and_const_coexist() {
    let out = run_php(
        r#"<?php
const A = 1;
define("B", 2);
echo A + B;
"#,
    );
    assert_eq!(out, "3");
}

// ============================================================================
// Default parameter values
// ============================================================================

#[test]
fn test_default_param_basic() {
    let out = run_php(
        r#"<?php
function greet($name = "World") {
    echo "Hello " . $name;
}
greet();
"#,
    );
    assert_eq!(out, "Hello World");
}

#[test]
fn test_default_param_override() {
    let out = run_php(
        r#"<?php
function greet($name = "World") {
    echo "Hello " . $name;
}
greet("PHP");
"#,
    );
    assert_eq!(out, "Hello PHP");
}

#[test]
fn test_default_param_multiple() {
    let out = run_php(
        r#"<?php
function add($a, $b = 10, $c = 20) {
    return $a + $b + $c;
}
echo add(1);
echo " ";
echo add(1, 2);
echo " ";
echo add(1, 2, 3);
"#,
    );
    assert_eq!(out, "31 23 6");
}

#[test]
fn test_default_param_null() {
    let out = run_php(
        r#"<?php
function test($x = null) {
    if ($x === null) {
        echo "null";
    } else {
        echo $x;
    }
}
test();
"#,
    );
    assert_eq!(out, "null");
}

#[test]
fn test_default_param_bool() {
    let out = run_php(
        r#"<?php
function check($verbose = false) {
    if ($verbose) {
        echo "verbose";
    } else {
        echo "quiet";
    }
}
check();
echo " ";
check(true);
"#,
    );
    assert_eq!(out, "quiet verbose");
}

#[test]
fn test_default_param_integer() {
    let out = run_php(
        r#"<?php
function repeat($str, $times = 3) {
    $result = "";
    for ($i = 0; $i < $times; $i++) {
        $result .= $str;
    }
    echo $result;
}
repeat("a");
echo " ";
repeat("b", 2);
"#,
    );
    assert_eq!(out, "aaa bb");
}

#[test]
fn test_default_param_in_class_method() {
    let out = run_php(
        r#"<?php
class Greeter {
    public function hello($name = "World") {
        echo "Hi " . $name;
    }
}
$g = new Greeter();
$g->hello();
echo " ";
$g->hello("PHP");
"#,
    );
    assert_eq!(out, "Hi World Hi PHP");
}

#[test]
fn test_default_param_in_closure() {
    let out = run_php(
        r#"<?php
$add = function($a, $b = 5) {
    return $a + $b;
};
echo $add(10);
echo " ";
echo $add(10, 20);
"#,
    );
    assert_eq!(out, "15 30");
}

#[test]
fn test_default_param_expression() {
    let out = run_php(
        r#"<?php
function test($x = 2 + 3) {
    echo $x;
}
test();
"#,
    );
    assert_eq!(out, "5");
}

#[test]
fn test_default_param_string_concat() {
    let out = run_php(
        r#"<?php
function test($prefix = "Hello" . " " . "World") {
    echo $prefix;
}
test();
"#,
    );
    assert_eq!(out, "Hello World");
}

// ============================================================================
// Edge cases
// ============================================================================

#[test]
fn test_const_with_function_call() {
    let out = run_php(
        r#"<?php
const SEPARATOR = "-";
function join_parts($a, $b) {
    return $a . SEPARATOR . $b;
}
echo join_parts("hello", "world");
"#,
    );
    assert_eq!(out, "hello-world");
}

#[test]
fn test_many_defaults() {
    let out = run_php(
        r#"<?php
function config($host = "localhost", $port = 3306, $db = "test") {
    echo $host . ":" . $port . "/" . $db;
}
config();
echo "|";
config("prod");
echo "|";
config("prod", 5432);
echo "|";
config("prod", 5432, "mydb");
"#,
    );
    assert_eq!(
        out,
        "localhost:3306/test|prod:3306/test|prod:5432/test|prod:5432/mydb"
    );
}

#[test]
fn test_define_with_variable() {
    let out = run_php(
        r#"<?php
$val = 42;
define("DYNAMIC", $val);
echo DYNAMIC;
"#,
    );
    assert_eq!(out, "42");
}

// ============================================================================
// Regression: P1 — side-effect defaults must NOT run when arg is passed
// ============================================================================

#[test]
fn test_default_side_effect_skipped_when_arg_passed() {
    // The default calls a function with a side effect (echo).
    // When arg IS passed, the default must NOT be evaluated.
    let out = run_php(
        r#"<?php
function side() {
    echo "SIDE";
    return 99;
}
function test($x = side()) {
    echo $x;
}
test(5);
"#,
    );
    // Must output "5" only — "SIDE" must NOT appear
    assert_eq!(out, "5");
}

#[test]
fn test_default_side_effect_runs_when_arg_omitted() {
    // When arg is NOT passed, the default expression IS evaluated
    let out = run_php(
        r#"<?php
function side() {
    echo "SIDE";
    return 99;
}
function test($x = side()) {
    echo $x;
}
test();
"#,
    );
    assert_eq!(out, "SIDE99");
}

#[test]
fn test_default_side_effect_mixed() {
    // Multiple calls: first with arg (no side effect), second without (side effect)
    let out = run_php(
        r#"<?php
$count = 0;
function counter() {
    echo "C";
    return 1;
}
function test($a, $b = counter()) {
    echo $a . $b;
}
test(1, 2);
echo "|";
test(3);
"#,
    );
    assert_eq!(out, "12|C31");
}

// ============================================================================
// Regression: P2 — define() with non-string name uses string coercion
// ============================================================================

#[test]
fn test_define_integer_name_coerces_to_string() {
    // PHP coerces integer name to string "123"
    let out = run_php(
        r#"<?php
define("123", "val");
echo defined("123") ? "yes" : "no";
echo " ";
echo constant("123");
"#,
    );
    assert_eq!(out, "yes val");
}

#[test]
fn empty_dynamic_constant_names_are_valid_and_redefine_normally() {
    let out = run_php(
        r#"<?php
set_error_handler(function($severity, $message, $file, $line) {
    echo $severity, ':', $message, ':', $line, "\n";
    return true;
});
var_dump(define("", "first"));
var_dump(defined(""));
var_dump(constant(""));
var_dump(define("", "second"));
var_dump(constant(""));
"#,
    );
    assert_eq!(
        out,
        concat!(
            "bool(true)\n",
            "bool(true)\n",
            "string(5) \"first\"\n",
            "2:Constant  already defined, this will be an error in PHP 9:9\n",
            "bool(false)\n",
            "string(5) \"first\"\n",
        )
    );
}

#[test]
fn null_dynamic_constant_names_are_deprecated_and_coerced_to_empty() {
    let out = run_php(
        r#"<?php
set_error_handler(function($severity, $message, $file, $line) {
    echo $severity, ':', $message, ':', $line, "\n";
    return true;
});
var_dump(define(null, "first"));
var_dump(defined(""));
var_dump(constant(""));
"#,
    );
    assert_eq!(
        out,
        concat!(
            "8192:define(): Passing null to parameter #1 ($constant_name) of type string is deprecated:6\n",
            "bool(true)\n",
            "bool(true)\n",
            "string(5) \"first\"\n",
        )
    );
}

#[test]
fn strict_define_rejects_null_instead_of_defining_the_empty_name() {
    let out = run_php(
        r#"<?php
declare(strict_types=1);
try {
    define(null, "value");
} catch (TypeError $error) {
    echo $error->getMessage(), "\n";
}
var_dump(defined(""));
"#,
    );
    assert_eq!(
        out,
        concat!(
            "define(): Argument #1 ($constant_name) must be of type string, null given\n",
            "bool(false)\n",
        )
    );
}

// ============================================================================
// Class property defaults — eval_const_expr coverage
// ============================================================================

#[test]
fn test_class_property_default_array_indexed() {
    let out = run_php(
        r#"<?php
class Config {
    public $items = [1, 2, 3];
}
$c = new Config();
echo count($c->items);
echo " ";
echo $c->items[1];
"#,
    );
    assert_eq!(out, "3 2");
}

#[test]
fn test_class_property_default_array_keyed() {
    let out = run_php(
        r#"<?php
class Config {
    public $opts = ["host" => "localhost", "port" => 3306];
}
$c = new Config();
echo $c->opts["host"];
echo ":";
echo $c->opts["port"];
"#,
    );
    assert_eq!(out, "localhost:3306");
}

#[test]
fn test_class_property_default_nested_array() {
    let out = run_php(
        r#"<?php
class C {
    public $data = [1, [2, 3]];
}
$c = new C();
echo count($c->data);
echo " ";
echo $c->data[0];
"#,
    );
    assert_eq!(out, "2 1");
}

#[test]
fn test_class_property_default_empty_array() {
    let out = run_php(
        r#"<?php
class C {
    public $items = [];
}
$c = new C();
echo count($c->items);
"#,
    );
    assert_eq!(out, "0");
}

#[test]
fn test_class_property_default_negative_int() {
    let out = run_php(
        r#"<?php
class C {
    public $x = -42;
}
$c = new C();
echo $c->x;
"#,
    );
    assert_eq!(out, "-42");
}

#[test]
fn test_class_property_default_negative_float() {
    let out = run_php(
        r#"<?php
class C {
    public $x = -3.14;
}
$c = new C();
echo $c->x;
"#,
    );
    assert_eq!(out, "-3.14");
}

#[test]
fn test_class_property_default_all_scalar_types() {
    let out = run_php(
        r#"<?php
class C {
    public $a = 42;
    public $b = 3.14;
    public $c = "hello";
    public $d = true;
    public $e = false;
    public $f = null;
}
$c = new C();
echo $c->a . " " . $c->b . " " . $c->c . " " . $c->d;
"#,
    );
    assert_eq!(out, "42 3.14 hello 1");
}

#[test]
fn test_class_property_default_function_call_is_compile_error() {
    let tokens = rphp::lexer::Lexer::new(
        r#"<?php
class C { public $x = strlen("hi"); }
"#,
    )
    .tokenize()
    .unwrap();
    let stmts = rphp::parser::Parser::new(tokens).parse().unwrap();
    let result = rphp::compiler::compile::Compiler::new().compile(&stmts);
    assert!(
        result.is_err(),
        "Function call in property default should be a compile error"
    );
}

// ============================================================================
// Constant in class property default resolves correctly
// ============================================================================

#[test]
fn test_const_in_class_property_default_resolves() {
    // User-defined constants from the same file are available in property defaults.
    let result = run_php(
        r#"<?php
const FOO = 42;
class C { public $x = FOO; }
$c = new C();
echo $c->x;
"#,
    );
    assert_eq!(result, "42");
}

// ============================================================================
// Regression: P2 — constant as function default (runtime eval — works fine)
// ============================================================================

#[test]
fn test_const_used_as_function_default() {
    // Constants work fine as function default expressions (evaluated at runtime)
    let out = run_php(
        r#"<?php
const DEFAULT_PORT = 8080;
function connect($port = DEFAULT_PORT) {
    echo $port;
}
connect();
echo " ";
connect(3000);
"#,
    );
    assert_eq!(out, "8080 3000");
}

// ── Namespace constant resolution ────────────────────────────────

#[test]
fn test_const_in_namespace_prescan() {
    // Constants defined inside a namespace block should be pre-scanned
    // and available for property defaults / forward references.
    let out = run_php(
        r#"<?php
namespace App\Config;
const VERSION = 42;
echo VERSION;
"#,
    );
    assert_eq!(out, "42");
}

#[test]
fn test_const_in_namespace_used_by_class() {
    // Constant inside namespace pre-scanned so class property default can reference it.
    let out = run_php(
        r#"<?php
namespace App;
const MAX = 100;

class Config {
    public $limit = MAX;
}

$c = new Config();
echo $c->limit;
"#,
    );
    assert_eq!(out, "100");
}

#[test]
fn test_global_and_source_magic_constants() {
    let out = run_php_with_source_context(
        "<?php\necho __LINE__ . '|' . __FILE__ . '|' . __DIR__ . '|' . __file__;",
        "/virtual/project/example.php",
        "/virtual/project",
    );
    assert_eq!(
        out,
        "2|/virtual/project/example.php|/virtual/project|/virtual/project/example.php"
    );
}

#[test]
fn public_php_platform_identity_is_consistent() {
    let output = run_php(
        r#"<?php
echo PHP_MAJOR_VERSION, ".", PHP_MINOR_VERSION, ".", PHP_RELEASE_VERSION, "|";
echo PHP_VERSION_ID, "|", PHP_VERSION, "|", phpversion(), "|";
echo PHP_INT_SIZE, "|", PHP_SAPI, "|", php_sapi_name(), "|";
var_dump(phpversion("missing"), extension_loaded("missing"));
"#,
    );

    assert_eq!(
        output,
        "8.5.0|80500|8.5.0|8.5.0|8|cli|cli|bool(false)\nbool(false)\n"
    );
}

#[test]
fn constant_version_return_does_not_register_dead_conditional_polyfills() {
    let output = run_php(
        r#"<?php
if (PHP_VERSION_ID >= 80000) {
    return;
}
if (!class_exists('ValueError', false)) {
    class ValueError extends Error {}
}
"#,
    );

    assert_eq!(output, "");
}

#[test]
fn platform_and_versioning_setup_contracts_match_php() {
    let output = run_php(
        r#"<?php
echo PHP_OS_FAMILY, "|", PHP_OS, "|";
var_dump(PHP_DEBUG);
var_dump(version_compare(PHP_VERSION, '8.1', '<'));
var_dump(version_compare('1.0-dev', '1.0a1'));
var_dump(version_compare('1.0RC1', '1.0rc1', 'eq'));
var_dump(version_compare('1.0', '1.0pl1', '<'));
var_dump(setlocale(LC_ALL, 'invalid'));
var_dump(setlocale(LC_NUMERIC, 'C'));
var_dump(gc_collect_cycles());
"#,
    );

    let expected_os = if cfg!(windows) {
        "Windows|WINNT|"
    } else if cfg!(target_os = "macos") {
        "Darwin|Darwin|"
    } else if cfg!(target_os = "freebsd") {
        "BSD|FreeBSD|"
    } else if cfg!(target_os = "openbsd") {
        "BSD|OpenBSD|"
    } else if cfg!(target_os = "netbsd") {
        "BSD|NetBSD|"
    } else if cfg!(target_os = "dragonfly") {
        "BSD|DragonFlyBSD|"
    } else if cfg!(target_os = "solaris") {
        "Solaris|SunOS|"
    } else if cfg!(any(target_os = "linux", target_os = "android")) {
        "Linux|Linux|"
    } else {
        "Unknown|Unknown|"
    };
    assert_eq!(
        output,
        format!(
            "{expected_os}bool(false)\nbool(false)\nint(-1)\nbool(true)\nbool(true)\nbool(false)\nstring(1) \"C\"\nint(0)\n"
        )
    );
}

#[test]
fn numeric_separators_follow_php_lexing_rules() {
    let output = run_php(
        r#"<?php
$sum = 0;
foreach ([1_000, 0xA_B, 0b1_01, 0o1_7] as $value) {
    $sum += $value;
}
echo $sum;
"#,
    );
    assert_eq!(output, "1191");
}

#[test]
fn source_magic_constants_are_available_in_declaration_defaults() {
    let out = run_php_with_source_context(
        r#"<?php
const SOURCE_ROOT = __DIR__;
class GeneratedPaths {
    public static $files = [
        'main' => __DIR__ . '/src/main.php',
        'self' => __FILE__,
    ];
}
echo SOURCE_ROOT . '|';
echo GeneratedPaths::$files['main'] . '|';
echo GeneratedPaths::$files['self'];
"#,
        "/virtual/project/generated.php",
        "/virtual/project",
    );
    assert_eq!(
        out,
        "/virtual/project|/virtual/project/src/main.php|/virtual/project/generated.php"
    );
}

#[test]
fn test_scope_magic_constants() {
    let out = run_php(
        r#"<?php
namespace Demo;
function probe() {
    echo __FUNCTION__ . '|' . __METHOD__ . '|' . __CLASS__ . '|' . __NAMESPACE__ . ';';
}
class Subject {
    public function probe() {
        echo __FUNCTION__ . '|' . __METHOD__ . '|' . __CLASS__ . '|' . __TRAIT__ . '|' . __NAMESPACE__ . ';';
    }
}
trait NamedTrait {
    public function traitProbe() {
        echo __FUNCTION__ . '|' . __METHOD__ . '|' . __TRAIT__ . ';';
    }
}
class UsesTrait { use NamedTrait; }
probe();
(new Subject())->probe();
(new UsesTrait())->traitProbe();
"#,
    );
    assert_eq!(
        out,
        "Demo\\probe|Demo\\probe||Demo;probe|Demo\\Subject::probe|Demo\\Subject||Demo;traitProbe|Demo\\NamedTrait::traitProbe|Demo\\NamedTrait;"
    );
}

#[test]
fn test_fully_qualified_builtin_constant_uses_global_lookup() {
    assert_eq!(
        run_php("<?php echo \\PHP_EOL === PHP_EOL ? 'yes' : 'no';"),
        "yes"
    );
}

#[test]
fn test_error_constants_are_available_in_property_defaults() {
    assert_eq!(
        run_php(
            "<?php class ErrorLevels { public array $levels = [E_ERROR, E_DEPRECATED, E_ALL]; } echo implode('|', (new ErrorLevels())->levels);"
        ),
        "1|8192|30719"
    );
}

#[test]
fn property_default_resolves_a_constant_from_its_own_class() {
    assert_eq!(
        run_php(
            "<?php class Status { public const INITIAL = 1; public int $value = self::INITIAL; } echo (new Status())->value;"
        ),
        "1"
    );
}

#[test]
fn included_property_default_resolves_an_imported_class_constant() {
    let nonce = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap()
        .as_nanos();
    let directory = std::env::temp_dir().join(format!(
        "rphp-cross-file-constant-{}-{nonce}",
        std::process::id()
    ));
    std::fs::create_dir_all(&directory).unwrap();
    std::fs::write(
        directory.join("Level.php"),
        "<?php namespace Fixture; class Level { public const INFO = 'info'; }",
    )
    .unwrap();
    std::fs::write(
        directory.join("Handler.php"),
        "<?php namespace Fixture; use Fixture\\Level as ImportedLevel; class Handler { public $level = ImportedLevel::INFO; }",
    )
    .unwrap();
    let main = directory.join("main.php");
    let source = "<?php require __DIR__ . '/Level.php'; require __DIR__ . '/Handler.php'; echo (new Fixture\\Handler())->level;";
    let out =
        run_php_with_source_context(source, main.to_str().unwrap(), directory.to_str().unwrap());
    std::fs::remove_dir_all(directory).unwrap();
    assert_eq!(out, "info");
}

#[test]
fn included_deprecated_constant_invalidates_a_warm_namespace_fallback_cache() {
    let nonce = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap()
        .as_nanos();
    let directory = std::env::temp_dir().join(format!(
        "rphp-included-deprecated-constant-{}-{nonce}",
        std::process::id()
    ));
    std::fs::create_dir_all(&directory).unwrap();
    std::fs::write(
        directory.join("deprecated.php"),
        "<?php namespace Fixture; #[\\Deprecated('use current')] const MARKER = 'included';",
    )
    .unwrap();
    let main = directory.join("main.php");
    let source = r#"<?php
namespace Fixture;
set_error_handler(function($level, $message) { echo "$level:$message|"; });
define('MARKER', 'fallback');
function readMarker() { echo MARKER, '|'; }
readMarker();
require __DIR__ . '/deprecated.php';
readMarker();
"#;
    let out =
        run_php_with_source_context(source, main.to_str().unwrap(), directory.to_str().unwrap());
    std::fs::remove_dir_all(directory).unwrap();
    assert_eq!(
        out,
        "fallback|16384:Constant Fixture\\MARKER is deprecated, use current|included|"
    );
}

#[test]
fn grouped_global_constants_are_defined_left_to_right() {
    assert_eq!(
        run_php("<?php const FIRST = 2, SECOND = FIRST + 3, THIRD = SECOND * 2; echo THIRD;"),
        "10"
    );
}

#[test]
fn invalid_class_constant_operation_is_not_deferred_by_an_unknown_constant() {
    let error = run_php_expect_error(
        "<?php class InvalidDeferredConstant { public const VALUE = UNKNOWN_CONSTANT + strlen('x'); }",
    )
    .to_string();
    assert!(
        error.contains("Cannot use non-constant expression as value for class constant"),
        "unexpected class-constant diagnostic: {error}"
    );
}

#[test]
fn deferred_class_constant_retains_its_source_file_magic_constant() {
    assert_eq!(
        run_php_with_source_context(
            "<?php define('RUNTIME_PART', '-ready'); class DeferredSource { public const VALUE = __FILE__ . RUNTIME_PART; } echo DeferredSource::VALUE;",
            "/virtual/deferred-source.php",
            "/virtual",
        ),
        "/virtual/deferred-source.php-ready"
    );
}
#[test]
fn relative_static_constant_expressions_use_php_specific_diagnostics() {
    for (source, expected) in [
        (
            "<?php class InvalidStaticClassName { const VALUE = static::class; }",
            "static::class cannot be used for compile-time class name resolution",
        ),
        (
            "<?php const INVALID_STATIC_FETCH = static::VALUE;",
            "\"static::\" is not allowed in compile-time constants",
        ),
        (
            "<?php class InvalidStaticCallable { const VALUE = static::method(...); }",
            "\"static\" is not allowed in compile-time constants",
        ),
    ] {
        let error = run_php_expect_error(source);
        assert!(
            error.to_string().contains(expected),
            "unexpected error: {error}"
        );
    }
}

#[test]
fn defined_constant_inventory_separates_core_and_user_values_without_shadowing_builtins() {
    assert_eq!(
        run_php(
            r#"<?php
$initial = get_defined_constants(true);
echo 'initial:', implode(',', array_keys($initial)), ':',
    (int) isset($initial['Core']['STDIN']), ':',
    (int) !isset($initial['user']), "\n";
define('INTROSPECTION_SECOND', 2);
define('INTROSPECTION_FIRST', 1);
$flat = get_defined_constants();
$grouped = get_defined_constants(true);
echo 'flat:', (int) isset($flat['PHP_VERSION']), ':',
    $flat['INTROSPECTION_FIRST'], $flat['INTROSPECTION_SECOND'], "\n";
echo 'groups:', implode(',', array_keys($grouped)), ':',
    (int) isset($grouped['Core']['PHP_VERSION']), ':',
    $grouped['user']['INTROSPECTION_FIRST'],
    $grouped['user']['INTROSPECTION_SECOND'], "\n";
echo 'user-order:', implode(',', array_keys($grouped['user'])), "\n";
set_error_handler(function($level, $message) { echo "$level:$message\n"; });
var_dump(define('PHP_VERSION', 'forged'));
restore_error_handler();
echo 'version:', PHP_VERSION, ':', zend_version(), "\n";
"#,
        ),
        concat!(
            "initial:Core:1:1\n",
            "flat:1:12\n",
            "groups:Core,user:1:12\n",
            "user-order:INTROSPECTION_SECOND,INTROSPECTION_FIRST\n",
            "2:Constant PHP_VERSION already defined, this will be an error in PHP 9\n",
            "bool(false)\n",
            "version:8.5.0:4.5.0\n",
        )
    );
}
