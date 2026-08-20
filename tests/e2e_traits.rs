mod common;
use common::{run_php, run_php_expect_error};

#[test]
fn semi_reserved_trait_methods_and_namespace_relative_precedence_execute() {
    assert_eq!(
        run_php(
            r#"<?php
namespace Fixture;
trait PreferredWords {
    public function insteadof() { return 'preferred'; }
    public function try() { return 'aliased'; }
}
trait OtherWords {
    public function insteadof() { return 'other'; }
}
class WordConsumer {
    use PreferredWords, OtherWords {
        PreferredWords::insteadof insteadof namespace\OtherWords;
        try as public and;
    }
    var $keyword = 'legacy';
    public function or() { return 'declared'; }
    public static function throw() { return 'static'; }
    public function __CLASS__() { return 'magic'; }
}
$consumer = new WordConsumer();
echo $consumer->insteadof(), '|', $consumer->and(), '|', $consumer->or(), '|';
echo WordConsumer::throw(), '|', $consumer->__CLASS__(), '|', $consumer->keyword;
"#,
        ),
        "preferred|aliased|declared|static|magic|legacy"
    );
}

#[test]
fn nested_trait_method_satisfies_an_interface_contract() {
    assert_eq!(
        run_php(
            "<?php interface Reader { public function read(): string; } trait InnerReader { public function read(): string { return 'ok'; } } trait OuterReader { use InnerReader; } class NestedReader implements Reader { use OuterReader; } echo (new NestedReader())->read();"
        ),
        "ok"
    );
}

#[test]
fn constant_php_version_branch_registers_only_the_selected_trait() {
    assert_eq!(
        run_php(
            "<?php if (PHP_VERSION_ID >= 80300) { trait VersionedTrait { public function version() { return 'new'; } } } else { trait VersionedTrait { public function version() { return 'compat'; } } } class VersionedConsumer { use VersionedTrait; } echo (new VersionedConsumer())->version();"
        ),
        "new"
    );
}

#[test]
fn trait_class_magic_constant_uses_each_nearest_composition_scope() {
    assert_eq!(
        run_php(
            r#"<?php
trait InnerClassName {
    public function instanceName() { echo __CLASS__, '|', __TRAIT__, '|', __METHOD__, "\n"; }
    public static function staticName() { echo __CLASS__, '|', __METHOD__, "\n"; }
    public function closureName() { return function () { echo __CLASS__, "\n"; }; }
    public static function staticClosureName() { return static function () { echo __CLASS__, '|', static::class, "\n"; }; }
    public function originalName() { echo __CLASS__, '|', __METHOD__, "\n"; }
    public function recursiveName() {
        echo __CLASS__, "\n";
        if (get_parent_class(__CLASS__) !== false) { parent::recursiveName(); }
    }
    private function privateName($caller) { echo $caller, '|', __CLASS__, '|', __METHOD__, "\n"; }
}
trait OuterClassName { use InnerClassName; }
class ClassNameBase {
    use OuterClassName { originalName as aliasName; }
    public function basePrivateName() { $this->privateName(__METHOD__); }
}
class ClassNameChild extends ClassNameBase {}
class ClassNameReuse extends ClassNameBase { use OuterClassName { originalName as aliasAgain; } }
class ClassNameOther { use InnerClassName; }

(new ClassNameBase)->instanceName();
(new ClassNameReuse)->instanceName();
(new ClassNameChild)->instanceName();
(new ClassNameOther)->instanceName();
(new ClassNameBase)->instanceName();
ClassNameChild::staticName();
ClassNameReuse::staticName();
ClassNameBase::staticName();
$childClosure = (new ClassNameChild)->closureName();
$reuseClosure = (new ClassNameReuse)->closureName();
$childClosure();
$reuseClosure();
(new ClassNameBase)->aliasName();
(new ClassNameReuse)->aliasAgain();
(new ClassNameReuse)->recursiveName();
ClassNameChild::staticClosureName()();
ClassNameReuse::staticClosureName()();
(new ClassNameChild)->basePrivateName();
"#,
        ),
        concat!(
            "ClassNameBase|InnerClassName|InnerClassName::instanceName\n",
            "ClassNameReuse|InnerClassName|InnerClassName::instanceName\n",
            "ClassNameBase|InnerClassName|InnerClassName::instanceName\n",
            "ClassNameOther|InnerClassName|InnerClassName::instanceName\n",
            "ClassNameBase|InnerClassName|InnerClassName::instanceName\n",
            "ClassNameBase|InnerClassName::staticName\n",
            "ClassNameReuse|InnerClassName::staticName\n",
            "ClassNameBase|InnerClassName::staticName\n",
            "ClassNameBase\n",
            "ClassNameReuse\n",
            "ClassNameBase|InnerClassName::originalName\n",
            "ClassNameReuse|InnerClassName::originalName\n",
            "ClassNameReuse\n",
            "ClassNameBase\n",
            "ClassNameBase|ClassNameChild\n",
            "ClassNameReuse|ClassNameReuse\n",
            "ClassNameBase::basePrivateName|ClassNameBase|InnerClassName::privateName\n",
        )
    );
}

// ─── Basic trait usage ────────────────────────────────────────────

#[test]
fn trait_basic_method() {
    let out = run_php(
        r#"<?php
trait Greet {
    public function hello() {
        echo "Hello from trait\n";
    }
}
class MyClass {
    use Greet;
}
$obj = new MyClass();
$obj->hello();
"#,
    );
    assert_eq!(out, "Hello from trait\n");
}

#[test]
fn trait_method_with_params() {
    let out = run_php(
        r#"<?php
trait MathTrait {
    public function add($a, $b) {
        return $a + $b;
    }
}
class Calc {
    use MathTrait;
}
$c = new Calc();
echo $c->add(3, 7);
"#,
    );
    assert_eq!(out, "10");
}

#[test]
fn trait_multiple_methods() {
    let out = run_php(
        r#"<?php
trait Logger {
    public function log($msg) {
        echo "LOG: " . $msg . "\n";
    }
    public function warn($msg) {
        echo "WARN: " . $msg . "\n";
    }
}
class App {
    use Logger;
}
$app = new App();
$app->log("started");
$app->warn("low memory");
"#,
    );
    assert_eq!(out, "LOG: started\nWARN: low memory\n");
}

// ─── Multiple traits ──────────────────────────────────────────────

#[test]
fn trait_use_multiple() {
    let out = run_php(
        r#"<?php
trait A {
    public function fromA() { echo "A"; }
}
trait B {
    public function fromB() { echo "B"; }
}
class C {
    use A, B;
}
$c = new C();
$c->fromA();
$c->fromB();
"#,
    );
    assert_eq!(out, "AB");
}

// ─── Trait with properties ────────────────────────────────────────

#[test]
fn trait_with_property() {
    let out = run_php(
        r#"<?php
trait HasName {
    public $name = "default";
}
class User {
    use HasName;
}
$u = new User();
echo $u->name . "\n";
$u->name = "Alice";
echo $u->name;
"#,
    );
    assert_eq!(out, "default\nAlice");
}

#[test]
fn trait_property_class_defaults_bind_at_each_composition_boundary() {
    assert_eq!(
        run_php(
            r#"<?php
class DirectPropertyDefaults {
    public $instance = __CLASS__;
    public static $static = __CLASS__;
}
trait InnerPropertyDefaults {
    public $instance = __CLASS__;
    public $pseudo = self::class;
    public $decorated = self::class . 'Suffix';
    public static $static = __CLASS__;
    public static $staticPseudo = self::class;
}
trait OuterPropertyDefaults { use InnerPropertyDefaults; }
class PropertyDefaultBase { use OuterPropertyDefaults; }
class PropertyDefaultChild extends PropertyDefaultBase {}
class PropertyDefaultReuse extends PropertyDefaultBase { use OuterPropertyDefaults; }
class PropertyDefaultOther { use InnerPropertyDefaults; }

$direct = new DirectPropertyDefaults();
echo $direct->instance, ':', DirectPropertyDefaults::$static, "\n";
$base = new PropertyDefaultBase();
echo $base->instance, ':', $base->pseudo, ':', $base->decorated, ':', PropertyDefaultBase::$static, ':', PropertyDefaultBase::$staticPseudo, "\n";
$child = new PropertyDefaultChild();
echo $child->instance, ':', $child->pseudo, ':', $child->decorated, ':', PropertyDefaultChild::$static, ':', PropertyDefaultChild::$staticPseudo, "\n";
$childDefaults = (new ReflectionClass(PropertyDefaultChild::class))->getDefaultProperties();
echo $childDefaults['instance'], ':', $childDefaults['pseudo'], ':', $childDefaults['decorated'], ':', $childDefaults['static'], ':', $childDefaults['staticPseudo'], "\n";
$reuse = new PropertyDefaultReuse();
echo $reuse->instance, ':', $reuse->pseudo, ':', $reuse->decorated, ':', PropertyDefaultReuse::$static, ':', PropertyDefaultReuse::$staticPseudo, "\n";
$other = new PropertyDefaultOther();
echo $other->instance, ':', $other->pseudo, ':', $other->decorated, ':', PropertyDefaultOther::$static, ':', PropertyDefaultOther::$staticPseudo;
"#,
        ),
        concat!(
            "DirectPropertyDefaults:DirectPropertyDefaults\n",
            "PropertyDefaultBase:PropertyDefaultBase:PropertyDefaultBaseSuffix:PropertyDefaultBase:PropertyDefaultBase\n",
            "PropertyDefaultBase:PropertyDefaultBase:PropertyDefaultBaseSuffix:PropertyDefaultBase:PropertyDefaultBase\n",
            "PropertyDefaultBase:PropertyDefaultBase:PropertyDefaultBaseSuffix:PropertyDefaultChild:PropertyDefaultChild\n",
            "PropertyDefaultReuse:PropertyDefaultReuse:PropertyDefaultReuseSuffix:PropertyDefaultReuse:PropertyDefaultReuse\n",
            "PropertyDefaultOther:PropertyDefaultOther:PropertyDefaultOtherSuffix:PropertyDefaultOther:PropertyDefaultOther",
        )
    );
}

#[test]
fn trait_property_class_default_collisions_use_the_current_composer() {
    assert_eq!(
        run_php(
            r#"<?php
trait DynamicFirst { public string $value = __CLASS__; }
trait LiteralSecond { public string $value = 'DynamicOuter'; }
trait DynamicOuter { use DynamicFirst, LiteralSecond; }
class DynamicConsumer { use DynamicOuter; }

trait LiteralFirst { public string $value = 'LiteralOuter'; }
trait DynamicSecond { public string $value = self::class; }
trait LiteralOuter { use LiteralFirst, DynamicSecond; }
class LiteralConsumer { use LiteralOuter; }

trait MagicSpelling { public string $value = __CLASS__; }
trait PseudoSpelling { public string $value = self::class; }
class SameConsumer { use MagicSpelling, PseudoSpelling; }

trait ExplicitTrait { public string $value = __CLASS__; }
class ExplicitConsumer {
    use ExplicitTrait;
    public string $value = 'ExplicitConsumer';
}

echo (new DynamicConsumer())->value, ':', (new LiteralConsumer())->value, ':';
echo (new SameConsumer())->value, ':', (new ExplicitConsumer())->value;
"#,
        ),
        "DynamicConsumer:LiteralOuter:SameConsumer:ExplicitConsumer"
    );
}

#[test]
fn private_trait_properties_use_the_consuming_class_scope() {
    let out = run_php(
        r#"<?php
trait PrivateTraitState {
    private $value = "trait";
    private static $shared = "static";
    public function readFromTrait() { return $this->value . ":" . self::$shared; }
}
class PrivateTraitConsumer {
    use PrivateTraitState;
    public function writeFromClass($value) { $this->value = $value; self::$shared = "class"; }
}
$consumer = new PrivateTraitConsumer();
$consumer->writeFromClass("instance");
echo $consumer->readFromTrait();
"#,
    );
    assert_eq!(out, "instance:class");
}

#[test]
fn shared_trait_body_uses_each_consuming_class_private_scope() {
    let out = run_php(
        r#"<?php
trait ReadsConsumerPrivateState {
    public function readPrivateState() { return $this->value; }
    public function writePrivateState($value) { $this->value = $value; }
}
class FirstPrivateConsumer {
    use ReadsConsumerPrivateState;
    public function __construct(private string $value) {}
}
class SecondPrivateConsumer {
    use ReadsConsumerPrivateState;
    public function __construct(private string $value) {}
}
$first = new FirstPrivateConsumer('first');
$first->writePrivateState('updated');
echo $first->readPrivateState();
echo ':';
echo (new SecondPrivateConsumer('second'))->readPrivateState();
"#,
    );
    assert_eq!(out, "updated:second");
}

// ─── Class method overrides trait ─────────────────────────────────

#[test]
fn trait_class_overrides_method() {
    let out = run_php(
        r#"<?php
trait Greet {
    public function hello() {
        echo "trait";
    }
}
class MyClass {
    use Greet;
    public function hello() {
        echo "class";
    }
}
$obj = new MyClass();
$obj->hello();
"#,
    );
    assert_eq!(out, "class");
}

// ─── Trait + inheritance ──────────────────────────────────────────

#[test]
fn trait_with_inheritance() {
    let out = run_php(
        r#"<?php
trait Greet {
    public function hello() {
        echo "trait hello\n";
    }
}
class Base {
    public function base_method() {
        echo "base\n";
    }
}
class Child extends Base {
    use Greet;
}
$c = new Child();
$c->base_method();
$c->hello();
"#,
    );
    assert_eq!(out, "base\ntrait hello\n");
}

#[test]
fn trait_overrides_parent_method() {
    let out = run_php(
        r#"<?php
trait Greet {
    public function hello() {
        echo "from trait";
    }
}
class Base {
    public function hello() {
        echo "from base";
    }
}
class Child extends Base {
    use Greet;
}
$c = new Child();
$c->hello();
"#,
    );
    assert_eq!(out, "from trait");
}

// ─── Trait satisfies interface ────────────────────────────────────

#[test]
fn trait_satisfies_interface() {
    let out = run_php(
        r#"<?php
interface Loggable {
    public function log($msg);
}
trait LogTrait {
    public function log($msg) {
        echo $msg;
    }
}
class App implements Loggable {
    use LogTrait;
}
$app = new App();
$app->log("works!");
"#,
    );
    assert_eq!(out, "works!");
}

// ─── Trait with $this ─────────────────────────────────────────────

#[test]
fn trait_method_accesses_this() {
    let out = run_php(
        r#"<?php
trait Greet {
    public function greet() {
        echo "Hello, " . $this->name;
    }
}
class Person {
    use Greet;
    public $name;
    public function __construct($name) {
        $this->name = $name;
    }
}
$p = new Person("Alice");
$p->greet();
"#,
    );
    assert_eq!(out, "Hello, Alice");
}

// ─── Static trait methods ─────────────────────────────────────────

#[test]
fn trait_static_method() {
    let out = run_php(
        r#"<?php
trait Counter {
    public static function count_to($n) {
        $i = 1;
        while ($i <= $n) {
            echo $i;
            $i = $i + 1;
        }
    }
}

class App {
    use Counter;
}
App::count_to(3);
"#,
    );
    assert_eq!(out, "123");
}

#[test]
fn trait_method_statics_are_isolated_per_alias_and_consumer() {
    assert_eq!(
        run_php(
            r#"<?php
trait CounterTrait {
    public static function counter() {
        static $count = 0;
        echo ++$count;
    }
}
class FirstCounter {
    use CounterTrait {
        CounterTrait::counter as firstAlias;
        CounterTrait::counter as secondAlias;
    }
}
class SecondCounter { use CounterTrait; }
FirstCounter::counter(); FirstCounter::firstAlias();
FirstCounter::secondAlias(); SecondCounter::counter();
FirstCounter::counter(); FirstCounter::firstAlias();
FirstCounter::secondAlias(); SecondCounter::counter();
"#
        ),
        "11112222"
    );
}

#[test]
fn trait_backtraces_use_composed_method_names_and_inherit_the_composer_identity() {
    assert_eq!(
        run_php(
            r#"<?php
trait TraceNames {
    public function instanceName() {
        $frame = debug_backtrace(DEBUG_BACKTRACE_IGNORE_ARGS, 1)[0];
        return $frame['class'] . '::' . $frame['function'] . $frame['type'];
    }
    public static function staticName() {
        $frame = debug_backtrace(DEBUG_BACKTRACE_IGNORE_ARGS, 1)[0];
        return $frame['class'] . '::' . $frame['function'] . $frame['type'];
    }
}
class PrimaryTrace {
    use TraceNames {
        instanceName as aliasName;
        staticName as aliasStatic;
    }
}
class ChildTrace extends PrimaryTrace {}
$primary = new PrimaryTrace();
$child = new ChildTrace();
echo $primary->instanceName(), '|', $primary->aliasName(), '|';
echo $child->instanceName(), '|', $child->aliasName(), '|';
echo PrimaryTrace::staticName(), '|', PrimaryTrace::aliasStatic(), '|';
echo ChildTrace::staticName(), '|', ChildTrace::aliasStatic();
"#,
        ),
        concat!(
            "PrimaryTrace::instanceName->|PrimaryTrace::aliasName->|",
            "PrimaryTrace::instanceName->|PrimaryTrace::aliasName->|",
            "PrimaryTrace::staticName::|PrimaryTrace::aliasStatic::|",
            "PrimaryTrace::staticName::|PrimaryTrace::aliasStatic::",
        )
    );
}

#[test]
fn trait_type_errors_use_composed_alias_and_consumer_names() {
    assert_eq!(
        run_php(
            r#"<?php
trait ComposedDiagnostics {
    public function accepts(int $value): int { return []; }
}
class DiagnosticConsumer {
    use ComposedDiagnostics { accepts as alias; }
}
class DiagnosticChild extends DiagnosticConsumer {}
try {
    (new DiagnosticConsumer())->alias([]);
} catch (TypeError $error) {
    echo explode('():', $error->getMessage())[0], '|';
}
try {
    (new DiagnosticConsumer())->accepts(1);
} catch (TypeError $error) {
    echo explode('():', $error->getMessage())[0], '|';
}
try {
    (new DiagnosticChild())->alias([]);
} catch (TypeError $error) {
    echo explode('():', $error->getMessage())[0], '|';
}
"#,
        ),
        concat!(
            "DiagnosticConsumer::alias|",
            "DiagnosticConsumer::accepts|",
            "DiagnosticConsumer::alias|",
        )
    );
}

#[test]
fn trait_static_pseudo_calls_resolve_for_each_consuming_class() {
    let out = run_php(
        r#"<?php
trait CallsScope {
    public static function selfValue(): string { return self::value(); }
    public static function parentValue(): string { return parent::value(); }
}
class FirstBase {
    public static function value(): string { return "first-base"; }
}
class First extends FirstBase {
    use CallsScope;
    public static function value(): string { return "first"; }
}
class SecondBase {
    public static function value(): string { return "second-base"; }
}
class Second extends SecondBase {
    use CallsScope;
    public static function value(): string { return "second"; }
}
echo First::selfValue() . ":" . First::parentValue() . ":";
echo Second::selfValue() . ":" . Second::parentValue() . ":";
echo First::selfValue() . ":" . First::parentValue();
"#,
    );
    assert_eq!(out, "first:first-base:second:second-base:first:first-base");
}

#[test]
fn trait_late_static_calls_follow_each_consuming_class() {
    let out = run_php(
        r#"<?php
trait LateTraitCall {
    public static function dispatch(): string { return static::value(); }
}
class LateTraitFirst {
    use LateTraitCall;
    public static function value(): string { return "first"; }
}
class LateTraitSecond {
    use LateTraitCall;
    public static function value(): string { return "second"; }
}
echo LateTraitFirst::dispatch() . ":";
echo LateTraitSecond::dispatch() . ":";
echo LateTraitFirst::dispatch();
"#,
    );
    assert_eq!(out, "first:second:first");
}

#[test]
fn trait_late_static_properties_follow_each_consuming_class() {
    let out = run_php(
        r#"<?php
trait LateTraitProperty {
    public static function read(): string { return static::$value; }
}
class LateTraitPropertyFirst {
    use LateTraitProperty;
    public static $value = "first";
}
class LateTraitPropertySecond {
    use LateTraitProperty;
    public static $value = "second";
}
echo LateTraitPropertyFirst::read() . ":";
echo LateTraitPropertySecond::read() . ":";
echo LateTraitPropertyFirst::read();
"#,
    );
    assert_eq!(out, "first:second:first");
}

#[test]
fn trait_static_property_storage_is_per_consumer_and_per_reuse() {
    let out = run_php(
        r#"<?php
trait MutableTraitStatic {
    public static $value = "trait";
    public static function write(string $value): void { static::$value = $value; }
}
class MutableTraitFirst { use MutableTraitStatic; }
class MutableTraitSecond { use MutableTraitStatic; }
class MutableTraitParent { use MutableTraitStatic; }
class MutableTraitChild extends MutableTraitParent { use MutableTraitStatic; }

MutableTraitFirst::write("first");
MutableTraitParent::write("parent");
echo MutableTraitFirst::$value . ":" . MutableTraitSecond::$value . ":";
echo MutableTraitParent::$value . ":" . MutableTraitChild::$value . ":";
MutableTraitChild::write("child");
echo MutableTraitParent::$value . ":" . MutableTraitChild::$value;
"#,
    );
    assert_eq!(out, "first:trait:parent:trait:parent:child");
}

#[test]
fn compatible_trait_static_properties_share_one_composed_class_slot() {
    let out = run_php(
        r#"<?php
trait FirstCompatibleStatic { public static $value = "same"; }
trait SecondCompatibleStatic { public static $value = "same"; }
class CompatibleStaticConsumer {
    use FirstCompatibleStatic, SecondCompatibleStatic;
}
CompatibleStaticConsumer::$value = "changed";
echo CompatibleStaticConsumer::$value;
"#,
    );
    assert_eq!(out, "changed");
}

#[test]
fn incompatible_trait_static_properties_are_rejected() {
    let result = std::panic::catch_unwind(|| {
        run_php(
            r#"<?php
trait FirstIncompatibleStatic { public static $value = "first"; }
trait SecondIncompatibleStatic { public static $value = "second"; }
class IncompatibleStaticConsumer {
    use FirstIncompatibleStatic, SecondIncompatibleStatic;
}
"#,
        )
    });
    assert!(
        result.is_err(),
        "Expected panic from incompatible trait static property defaults"
    );
}

// ─── Trait property collision edge cases ──────────────────────────

#[test]
fn trait_property_same_default_ok() {
    // Two traits with same property, same visibility, same default → ok
    let out = run_php(
        r#"<?php
trait T1 { public $x = 1; }
trait T2 { public $x = 1; }
class C {
    use T1, T2;
}
$c = new C();
echo $c->x;
"#,
    );
    assert_eq!(out, "1");
}

#[test]
fn trait_property_different_default_rejected() {
    // Two traits with same property, same visibility, different default → error
    let result = std::panic::catch_unwind(|| {
        run_php(
            r#"<?php
trait T1 { public $x = 1; }
trait T2 { public $x = 2; }
class C {
    use T1, T2;
}
"#,
        )
    });
    assert!(
        result.is_err(),
        "Expected panic from incompatible trait property defaults"
    );
}

#[test]
fn trait_property_different_visibility_rejected() {
    // Two traits with same property but different visibility → error
    let result = std::panic::catch_unwind(|| {
        run_php(
            r#"<?php
trait T1 { public $x = 1; }
trait T2 { protected $x = 1; }
class C {
    use T1, T2;
}
"#,
        )
    });
    assert!(
        result.is_err(),
        "Expected panic from incompatible trait property visibility"
    );
}

#[test]
fn trait_property_class_overrides_trait() {
    // Class's own property always takes precedence over trait's
    let out = run_php(
        r#"<?php
trait T1 { public $x = 10; }
class C {
    use T1;
    public $x = 99;
}
$c = new C();
echo $c->x;
"#,
    );
    assert_eq!(out, "99");
}

#[test]
fn trait_property_string_default_same_ok() {
    // String defaults that are equal → ok
    let out = run_php(
        r#"<?php
trait T1 { public $name = "hello"; }
trait T2 { public $name = "hello"; }
class C {
    use T1, T2;
}
$c = new C();
echo $c->name;
"#,
    );
    assert_eq!(out, "hello");
}

#[test]
fn trait_property_string_default_different_rejected() {
    // String defaults that differ → error
    let result = std::panic::catch_unwind(|| {
        run_php(
            r#"<?php
trait T1 { public $name = "hello"; }
trait T2 { public $name = "world"; }
class C {
    use T1, T2;
}
"#,
        )
    });
    assert!(
        result.is_err(),
        "Expected panic from incompatible string defaults"
    );
}

#[test]
fn trait_property_null_vs_value_rejected() {
    // One trait has null default, other has int default → error
    let result = std::panic::catch_unwind(|| {
        run_php(
            r#"<?php
trait T1 { public $x = null; }
trait T2 { public $x = 0; }
class C {
    use T1, T2;
}
"#,
        )
    });
    assert!(
        result.is_err(),
        "Expected panic from null vs value default mismatch"
    );
}

#[test]
fn trait_property_array_default_same_ok() {
    // Two traits with same array default → compatible
    let out = run_php(
        r#"<?php
trait T1 { public $items = [1, 2, 3]; }
trait T2 { public $items = [1, 2, 3]; }
class C {
    use T1, T2;
}
$c = new C();
echo count($c->items);
"#,
    );
    assert_eq!(out, "3");
}

#[test]
fn trait_property_array_default_different_rejected() {
    // Two traits with different array defaults → error
    let result = std::panic::catch_unwind(|| {
        run_php(
            r#"<?php
trait T1 { public $items = [1, 2]; }
trait T2 { public $items = [1, 3]; }
class C {
    use T1, T2;
}
"#,
        )
    });
    assert!(
        result.is_err(),
        "Expected panic from incompatible array defaults"
    );
}

#[test]
fn trait_property_array_different_length_rejected() {
    let result = std::panic::catch_unwind(|| {
        run_php(
            r#"<?php
trait T1 { public $x = [1]; }
trait T2 { public $x = [1, 2]; }
class C {
    use T1, T2;
}
"#,
        )
    });
    assert!(
        result.is_err(),
        "Expected panic from different length array defaults"
    );
}

#[test]
fn trait_method_alias_can_change_visibility_and_name() {
    let out = run_php(
        r#"<?php
trait ReadsValue {
    public function get() { return 'aliased'; }
}
class Reader {
    use ReadsValue {
        ReadsValue::get as private doGet;
    }
    public function read() { return $this->doGet(); }
}
echo (new Reader())->read();
"#,
    );
    assert_eq!(out, "aliased");

    let err = run_php_expect_error(
        r#"<?php
trait ReadsValue {
    public function get() { return 'aliased'; }
}
class Reader {
    use ReadsValue {
        ReadsValue::get as private doGet;
    }
}
(new Reader())->doGet();
"#,
    );
    match err {
        rphp::vm::execute::VmError::Fatal(message) => {
            assert!(message.contains("private"), "unexpected error: {message}");
        }
        other => panic!("Expected private visibility error, got: {other:?}"),
    }
}

#[test]
fn trait_can_compose_another_trait() {
    assert_eq!(
        run_php(
            r#"<?php
trait InnerGreeting { public function greeting() { return 'nested'; } }
trait OuterGreeting {
    use InnerGreeting { greeting as private nestedGreeting; }
    public function greeting() { return $this->nestedGreeting(); }
}
class NestedGreeter { use OuterGreeting; }
echo (new NestedGreeter())->greeting();
"#,
        ),
        "nested"
    );
}
