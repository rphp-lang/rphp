/// Tests for class inheritance (extends)
mod common;
use common::{run_php, run_php_expect_error, run_php_with_source_context};
use rphp::compiler::compile::Compiler;
use rphp::lexer::Lexer;
use rphp::parser::Parser;
use rphp::vm::opcode::OpCode;

#[test]
fn override_methods_require_an_effective_inherited_contract() {
    assert_eq!(
        run_php(
            r#"<?php
interface NamedRequirement { public function fromInterface(): void; }
trait AbstractRequirement { abstract protected function fromTrait(): void; }
abstract class OverrideParent {
    abstract public function __construct();
    protected function inherited(): void {}
}
class OverrideChild extends OverrideParent implements NamedRequirement {
    use AbstractRequirement;
    #[Override] public function __construct() {}
    #[Override] public function inherited(): void {}
    #[Override] public function fromInterface(): void {}
    #[Override] public function fromTrait(): void {}
}

class TraitParent { protected function fromComposedTrait(): void {} }
trait ParentOverride {
    #[Override] public function fromComposedTrait(): void {}
}
class TraitChild extends TraitParent { use ParentOverride; }

trait SuppressedOverride {
    #[Override] public function suppressed(): void {}
}
trait SelectedPlain { public function suppressed(): void {} }
class PrecedenceChild {
    use SuppressedOverride, SelectedPlain {
        SelectedPlain::suppressed insteadof SuppressedOverride;
    }
}
echo 'ok';
"#,
        ),
        "ok"
    );

    let cases = [
        (
            "<?php class MissingOverride { #[Override] public function value(): void {} }",
            "MissingOverride::value()",
        ),
        (
            "<?php class PrivateParent { private function value(): void {} } class PrivateChild extends PrivateParent { #[Override] public function value(): void {} }",
            "PrivateChild::value()",
        ),
        (
            "<?php class ConstructorParent { public function __construct() {} } class ConstructorChild extends ConstructorParent { #[Override] public function __construct() {} }",
            "ConstructorChild::__construct()",
        ),
        (
            "<?php trait ConcreteMethod { public function value(): void {} } class ConcreteTraitChild { use ConcreteMethod; #[Override] public function value(): void {} }",
            "ConcreteTraitChild::value()",
        ),
        (
            "<?php class HiddenParent { private function hidden(): void {} } trait HiddenOverride { #[Override] public function hidden(): void {} } class HiddenTraitChild extends HiddenParent { use HiddenOverride; }",
            "HiddenTraitChild::hidden()",
        ),
        (
            "<?php class ForwardChild extends ForwardParent {} class ForwardParent { #[Override] public function missing(): void {} }",
            "ForwardParent::missing()",
        ),
        (
            "<?php trait AliasedOverride { #[Override] public function inherited(): void {} } class AliasParent { public function inherited(): void {} } class AliasChild extends AliasParent { use AliasedOverride { AliasedOverride::inherited as missingAlias; } }",
            "AliasChild::missingAlias()",
        ),
    ];
    for (source, member) in cases {
        let error = run_php_expect_error(source);
        assert_eq!(
            format!("{error:?}"),
            format!(
                "Fatal(\"{member} has #[\\\\Override] attribute, but no matching parent method exists\")"
            )
        );
    }
}

#[test]
fn runtime_class_alias_completes_pending_property_invariance_linking() {
    assert_eq!(
        run_php(
            r#"<?php
class OriginalType {}
class ParentBox { public OriginalType $value; }
var_dump(class_exists('ChildBox', false));
class_alias('OriginalType', 'AliasType');
class ChildBox extends ParentBox { public AliasType $value; }
var_dump(class_exists('ChildBox', false));
"#,
        ),
        "bool(false)\nbool(true)\n"
    );

    let error = run_php_expect_error(
        r#"<?php
class ExpectedType {}
class OtherType {}
class ParentBox { public ExpectedType $value; }
class_alias('OtherType', 'AliasType');
class ChildBox extends ParentBox { public AliasType $value; }
"#,
    );
    assert_eq!(
        format!("{error:?}"),
        "Fatal(\"Type of ChildBox::$value must be ExpectedType (as in class ParentBox)\")"
    );
}

#[test]
fn unresolved_pending_property_invariance_still_fails() {
    let error = run_php_expect_error(
        r#"<?php
class Known {}
class ParentClass { public Known $property; }
class ChildClass extends ParentClass { public Missing $property; }
"#,
    );
    assert_eq!(
        format!("{error:?}"),
        "Fatal(\"Type of ChildClass::$property must be Known (as in class ParentClass)\")"
    );
}

#[test]
fn property_invariance_reduces_inheritance_intersections_and_iterable() {
    assert_eq!(
        run_php(
            r#"<?php
class ParentType {}
class ChildType extends ParentType {}
interface Marker {}
class ParentBox {
    public ParentType&ChildType $reduced;
    public iterable $iterable;
}
class ChildBox extends ParentBox {
    public ChildType $reduced;
    public array|Traversable $iterable;
}
echo "ok\n";
"#,
        ),
        "ok\n"
    );
}

#[test]
fn static_asymmetric_visibility_guards_all_write_forms_but_not_nested_objects() {
    assert_eq!(
        run_php(
            r#"<?php
class StaticBox {
    public private(set) static int $value = 1;
    public private(set) static array $values = [];
    public private(set) static object $node;
    public private(set) static object $unset;
    public static function initialize(): void {
        self::$value = 2;
        self::$node = new stdClass();
    }
}
StaticBox::initialize();
var_dump(StaticBox::$value);
try { StaticBox::$value = 3; } catch (Error $error) { echo $error->getMessage(), "\n"; }
try { ++StaticBox::$value; } catch (Error $error) { echo $error->getMessage(), "\n"; }
try { StaticBox::$values[] = 4; } catch (Error $error) { echo $error->getMessage(), "\n"; }
try { $value = &StaticBox::$value; } catch (Error $error) { echo $error->getMessage(), "\n"; }
StaticBox::$node->answer = 42;
var_dump(StaticBox::$node->answer);
unset(StaticBox::$unset->answer);
"#,
        ),
        "int(2)\nCannot modify private(set) property StaticBox::$value from global scope\nCannot indirectly modify private(set) property StaticBox::$value from global scope\nCannot indirectly modify private(set) property StaticBox::$values from global scope\nCannot indirectly modify private(set) property StaticBox::$value from global scope\nint(42)\n"
    );
}

#[test]
fn test_extends_basic() {
    assert_eq!(
        run_php(
            r#"<?php
class Animal {
    public $name;
    public function speak() {
        return "...";
    }
}
class Dog extends Animal {
    public function speak() {
        return "Woof";
    }
}
$d = new Dog();
$d->name = "Rex";
echo $d->name . " says " . $d->speak();
"#
        ),
        "Rex says Woof"
    );
}

#[test]
fn test_extends_inherits_method() {
    assert_eq!(
        run_php(
            r#"<?php
class Base {
    public function hello() {
        return "Hello";
    }
}
class Child extends Base {}
$c = new Child();
echo $c->hello();
"#
        ),
        "Hello"
    );
}

#[test]
fn static_self_and_parent_calls_use_lexical_class_scope() {
    assert_eq!(
        run_php(
            r#"<?php
namespace StaticScope;

class Base {
    public static function value(): int { return 1; }
}

class Child extends Base {
    public static function own(): int { return 2; }
    public static function calls(): int {
        return self::own() + self::value() + parent::value();
    }
}

echo Child::calls();
"#
        ),
        "4"
    );
}

#[test]
fn late_static_calls_follow_and_rekey_the_called_class() {
    assert_eq!(
        run_php(
            r#"<?php
class LateRoot {
    public static function value(): string { return "R"; }
    public static function dispatch(): string { return static::value(); }
    public function instanceDispatch(): string { return static::value(); }
}
class LateLeft extends LateRoot {
    public static function value(): string { return "L"; }
}
class LateRight extends LateRoot {
    public static function value(): string { return "X"; }
}

echo LateRoot::dispatch();
echo LateLeft::dispatch();
echo LateRight::dispatch();
echo LateLeft::dispatch();
$right = new LateRight();
echo $right->instanceDispatch();
"#
        ),
        "RLXLX"
    );
}

#[test]
fn get_called_class_reuses_late_static_identity_across_forwarding_callbacks_and_aliases() {
    assert_eq!(
        run_php(
            r#"<?php
class CalledBase {
    public static function direct() { echo get_called_class(), "\n"; }
    public static function forwarded() { static::direct(); }
    public function instance() { echo get_called_class(), "\n"; }
}
class CalledChild extends CalledBase {}
class_alias("CalledBase", "CalledAlias");
CalledBase::direct();
CalledChild::direct();
CalledChild::forwarded();
call_user_func([CalledChild::class, "direct"]);
(new CalledChild)->instance();
CalledAlias::direct();
try { get_called_class(); } catch (Error $error) { echo $error->getMessage(), "\n"; }
"#,
        ),
        "CalledBase\nCalledChild\nCalledChild\nCalledChild\nCalledChild\nCalledBase\nget_called_class() must be called from within a class\n"
    );
}

#[test]
fn get_parent_class_handles_lexical_trait_object_alias_and_invalid_inputs() {
    assert_eq!(
        run_php(
            r#"<?php
error_reporting(E_ALL & ~E_DEPRECATED);
class ParentProbe {}
trait ParentTrait { public function parentFromTrait() { var_dump(get_parent_class()); } }
class ChildProbe extends ParentProbe {
    use ParentTrait;
    public function parentFromMethod() { var_dump(get_parent_class()); }
}
class_alias("ChildProbe", "ChildAlias");
$child = new ChildProbe;
$child->parentFromMethod();
$child->parentFromTrait();
var_dump(get_parent_class($child));
var_dump(get_parent_class("ChildAlias"));
var_dump(get_parent_class("\\ChildAlias"));
var_dump(get_parent_class("ParentProbe"));
var_dump(get_parent_class());
foreach (["MissingParentProbe", "", [], 1, null] as $invalid) {
    try { get_parent_class($invalid); } catch (TypeError $error) { echo $error->getMessage(), "\n"; }
}
"#,
        ),
        concat!(
            "string(11) \"ParentProbe\"\nstring(11) \"ParentProbe\"\n",
            "string(11) \"ParentProbe\"\nstring(11) \"ParentProbe\"\nstring(11) \"ParentProbe\"\nbool(false)\nbool(false)\n",
            "get_parent_class(): Argument #1 ($object_or_class) must be an object or a valid class name, string given\n",
            "get_parent_class(): Argument #1 ($object_or_class) must be an object or a valid class name, string given\n",
            "get_parent_class(): Argument #1 ($object_or_class) must be an object or a valid class name, array given\n",
            "get_parent_class(): Argument #1 ($object_or_class) must be an object or a valid class name, int given\n",
            "get_parent_class(): Argument #1 ($object_or_class) must be an object or a valid class name, null given\n",
        )
    );
}

#[test]
fn get_parent_class_without_argument_deprecates_before_php_85_lexical_lookup() {
    assert_eq!(
        run_php_with_source_context(
            r#"<?php
class ParentName {
    public static function noParent() { var_dump(get_parent_class()); }
}
class ChildParentName extends ParentName {
    public static function inherited() { var_dump(get_parent_class()); }
    public static function explicit() { var_dump(get_parent_class(static::class)); }
}
class LeafParentName extends ChildParentName {}
set_error_handler(function($level, $message) { throw new Exception($message); });
try { get_parent_class(); } catch (Exception $error) { echo 'global:', $error->getMessage(), "\n"; }
try { LeafParentName::inherited(); } catch (Exception $error) { echo 'method:', $error->getMessage(), "\n"; }
restore_error_handler();
ParentName::noParent();
LeafParentName::inherited();
LeafParentName::explicit();
"#,
            "/virtual/get-parent-class.php",
            "/virtual",
        ),
        concat!(
            "global:Calling get_parent_class() without arguments is deprecated\n",
            "method:Calling get_parent_class() without arguments is deprecated\n",
            "\nDeprecated: Calling get_parent_class() without arguments is deprecated in /virtual/get-parent-class.php on line 3\n",
            "bool(false)\n",
            "\nDeprecated: Calling get_parent_class() without arguments is deprecated in /virtual/get-parent-class.php on line 6\n",
            "string(10) \"ParentName\"\n",
            "string(15) \"ChildParentName\"\n",
        )
    );
}

#[test]
fn get_class_methods_filters_effective_methods_by_lexical_scope() {
    assert_eq!(
        run_php(
            r#"<?php
trait MethodInventoryTrait {
    private function hidden() {}
    protected function guarded() {}
    public function open() {}
}

class MethodInventoryParent {
    private function parentPrivate() {}
    protected function parentProtected() {}
    public function inherited() {}
    public function shadow() {}
}

class MethodInventoryChild extends MethodInventoryParent {
    use MethodInventoryTrait {
        open as protected aliasOpen;
        guarded as public aliasGuarded;
    }
    public function own() {}
    public function shadow() {}
    public function inspect() { var_dump(get_class_methods(self::class)); }
}
var_dump(get_class_methods("MethodInventoryChild"));
(new MethodInventoryChild)->inspect();
class_alias("MethodInventoryChild", "MethodInventoryAlias");
var_dump(get_class_methods("MethodInventoryAlias"));
foreach (["MissingMethodInventory", "", [], 1, null] as $invalid) {
    try { get_class_methods($invalid); } catch (TypeError $error) { echo $error->getMessage(), "\n"; }
}
"#,
        ),
        concat!(
            "array(6) {\n  [0]=>\n  string(3) \"own\"\n  [1]=>\n  string(6) \"shadow\"\n  [2]=>\n  string(7) \"inspect\"\n  [3]=>\n  string(9) \"inherited\"\n  [4]=>\n  string(12) \"aliasGuarded\"\n  [5]=>\n  string(4) \"open\"\n}\n",
            "array(10) {\n  [0]=>\n  string(3) \"own\"\n  [1]=>\n  string(6) \"shadow\"\n  [2]=>\n  string(7) \"inspect\"\n  [3]=>\n  string(15) \"parentProtected\"\n  [4]=>\n  string(9) \"inherited\"\n  [5]=>\n  string(6) \"hidden\"\n  [6]=>\n  string(12) \"aliasGuarded\"\n  [7]=>\n  string(7) \"guarded\"\n  [8]=>\n  string(9) \"aliasOpen\"\n  [9]=>\n  string(4) \"open\"\n}\n",
            "array(6) {\n  [0]=>\n  string(3) \"own\"\n  [1]=>\n  string(6) \"shadow\"\n  [2]=>\n  string(7) \"inspect\"\n  [3]=>\n  string(9) \"inherited\"\n  [4]=>\n  string(12) \"aliasGuarded\"\n  [5]=>\n  string(4) \"open\"\n}\n",
            "get_class_methods(): Argument #1 ($object_or_class) must be an object or a valid class name, string given\n",
            "get_class_methods(): Argument #1 ($object_or_class) must be an object or a valid class name, string given\n",
            "get_class_methods(): Argument #1 ($object_or_class) must be an object or a valid class name, array given\n",
            "get_class_methods(): Argument #1 ($object_or_class) must be an object or a valid class name, int given\n",
            "get_class_methods(): Argument #1 ($object_or_class) must be an object or a valid class name, null given\n",
        )
    );
}

#[test]
fn get_class_vars_returns_visible_declaration_defaults() {
    assert_eq!(
        run_php(
            r#"<?php
class VarsParent {
    public $pub = 1;
    protected $prot = 2;
    private $priv = 3;
    public static int $typed;
    public static $changed = 4;
}
class VarsChild extends VarsParent {
    public $child = 5;
    private static $secret = 6;
    public function inspect() {
        var_export(get_class_vars(self::class)); echo "\n";
        var_export(get_class_vars(VarsParent::class)); echo "\n";
    }
}
VarsParent::$changed = 99;
var_export(get_class_vars('VarsChild')); echo "\n";
(new VarsChild)->inspect();
class_alias(VarsChild::class, 'VarsAlias');
var_export(get_class_vars('\\VarsAlias')); echo "\n";
try { get_class_vars('MissingVars'); } catch (TypeError $error) { echo $error->getMessage(), "\n"; }
"#,
        ),
        concat!(
            "array (\n  'child' => 5,\n  'pub' => 1,\n  'typed' => NULL,\n  'changed' => 4,\n)\n",
            "array (\n  'child' => 5,\n  'pub' => 1,\n  'prot' => 2,\n  'secret' => 6,\n  'typed' => NULL,\n  'changed' => 4,\n)\n",
            "array (\n  'pub' => 1,\n  'prot' => 2,\n  'typed' => NULL,\n  'changed' => 4,\n)\n",
            "array (\n  'child' => 5,\n  'pub' => 1,\n  'typed' => NULL,\n  'changed' => 4,\n)\n",
            "get_class_vars(): Argument #1 ($class) must be a valid class name, MissingVars given\n",
        )
    );
}

#[test]
fn inherited_private_property_reads_are_undefined_outside_the_owner_scope() {
    assert_eq!(
        run_php(
            r#"<?php
class PrivateReadParent {
    private $value = 'parent';
    public function readFromOwner(object $target) { var_dump($target->value); }
}
class PrivateReadChild extends PrivateReadParent {
    public function readFromChild(object $target) { var_dump($target->value); }
}
$parent = new PrivateReadParent;
$child = new PrivateReadChild;
set_error_handler(function ($level, $message) { echo "warning:$message\n"; return true; });
var_dump($child->value);
$child->readFromChild($child);
$parent->readFromOwner($child);
"#,
        ),
        concat!(
            "warning:Undefined property: PrivateReadChild::$value\nNULL\n",
            "warning:Undefined property: PrivateReadChild::$value\nNULL\n",
            "string(6) \"parent\"\n",
        )
    );

    let error = run_php_expect_error(
        r#"<?php
class PrivateReadExactParent { private $value = 'parent'; }
class PrivateReadExactChild extends PrivateReadExactParent {
    public function read(object $target) { return $target->value; }
}
(new PrivateReadExactChild)->read(new PrivateReadExactParent);
"#,
    );
    assert!(
        format!("{error:?}")
            .contains("Cannot access private property PrivateReadExactParent::$value")
    );
}

#[test]
fn inaccessible_instance_property_errors_are_catchable_and_keep_the_opcode_origin() {
    let source = r#"<?php
class HiddenProperty { private int $value = 1; }
$object = new HiddenProperty;
try { $read = $object->value; } catch (Error $error) { echo "read:", $error->getLine(), "\n"; }
try { $object->value = 2; } catch (Error $error) { echo "write:", $error->getLine(), "\n"; }
try { $reference =& $object->value; } catch (Error $error) { echo "reference:", $error->getLine(); }
"#;
    let tokens = Lexer::new(source).tokenize().unwrap();
    let statements = Parser::new(tokens).parse().unwrap();
    let compiled = Compiler::new().compile(&statements).unwrap();
    let property_lines: Vec<_> = compiled
        .main
        .instructions()
        .iter()
        .enumerate()
        .filter(|(_, instruction)| {
            matches!(
                instruction.opcode,
                OpCode::AssignObjProp | OpCode::BindObjPropRef
            )
        })
        .map(|(index, _)| compiled.main.source_line(index))
        .collect();
    assert!(property_lines.contains(&Some(5)), "{property_lines:?}");
    assert!(property_lines.contains(&Some(6)), "{property_lines:?}");

    assert_eq!(
        run_php_with_source_context(source, "/fixture/property-visibility.php", "/fixture",),
        concat!("read:4\n", "write:5\n", "reference:6",)
    );
}

#[test]
fn parent_instance_call_forwards_this_receiver() {
    assert_eq!(
        run_php(
            r#"<?php
class ParentReceiver {
    public function __construct(protected string $value) {}
    public function read(string $suffix): string { return $this->value . $suffix; }
}
class ChildReceiver extends ParentReceiver {
    public function readFromParent(): string { return parent::read('!'); }
}
echo (new ChildReceiver('forwarded'))->readFromParent();
"#
        ),
        "forwarded!"
    );
}

#[test]
fn late_static_properties_follow_and_rekey_the_called_class() {
    assert_eq!(
        run_php(
            r#"<?php
namespace LateProperties;

class Root {
    public static $value = "R";
    public static function late(): string { return static::$value; }
    public static function lexical(): string { return self::$value; }
    public function instanceLate(): string { return static::$value; }
}
class Left extends Root {
    public static $value = "L";
}
class Right extends Root {
    public static $value = "X";
}

echo Root::late();
echo Left::late();
echo Right::late();
echo Left::late();
echo Right::lexical();
$right = new Right();
echo $right->instanceLate();
"#
        ),
        "RLXLRX"
    );
}

#[test]
fn closures_capture_late_static_property_scope() {
    assert_eq!(
        run_php(
            r#"<?php
class PropertyClosureRoot {
    public static $value = "R";
    public static function make() {
        return fn(): string => static::$value;
    }
}
class PropertyClosureChild extends PropertyClosureRoot {
    public static $value = "C";
}
$root = PropertyClosureRoot::make();
$child = PropertyClosureChild::make();
echo $root() . $child() . $root();
"#
        ),
        "RCR"
    );
}

#[test]
fn late_static_property_visibility_uses_lexical_caller_scope() {
    assert_eq!(
        run_php(
            r#"<?php
class ProtectedPropertyRoot {
    protected static $value = "root";
    public static function read(): string { return static::$value; }
}
class ProtectedPropertyChild extends ProtectedPropertyRoot {
    protected static $value = "child";
}
echo ProtectedPropertyRoot::read() . ":" . ProtectedPropertyChild::read();
"#
        ),
        "root:child"
    );

    let error = run_php_expect_error(
        r#"<?php
class PrivatePropertyRoot {
    private static $value = "root";
    public static function read(): string { return static::$value; }
}
class PrivatePropertyChild extends PrivatePropertyRoot {
    private static $value = "child";
}
PrivatePropertyChild::read();
"#,
    );
    let rendered = format!("{error:?}");
    assert!(
        rendered.contains("Cannot access private property PrivatePropertyChild::$value"),
        "{rendered:?}"
    );
}

#[test]
fn mutable_static_properties_share_only_inherited_storage() {
    assert_eq!(
        run_php(
            r#"<?php
class MutableStaticRoot {
    public static $value = "root";
}
class MutableStaticInherited extends MutableStaticRoot {}
class MutableStaticRedeclared extends MutableStaticRoot {
    public static $value = "redeclared";
}

MutableStaticInherited::$value = "shared";
echo MutableStaticRoot::$value . ":" . MutableStaticInherited::$value . ":";
MutableStaticRedeclared::$value = "separate";
echo MutableStaticRoot::$value . ":" . MutableStaticRedeclared::$value;
"#,
        ),
        "shared:shared:shared:separate"
    );
}

#[test]
fn late_static_property_assignment_rekeys_and_self_remains_lexical() {
    assert_eq!(
        run_php(
            r#"<?php
class MutableLateRoot {
    public static $value = "root";
    public static function lateWrite(string $value): void { static::$value = $value; }
    public static function selfWrite(string $value): void { self::$value = $value; }
    public function instanceWrite(string $value): void { static::$value = $value; }
}
class MutableLateLeft extends MutableLateRoot { public static $value = "left"; }
class MutableLateRight extends MutableLateRoot { public static $value = "right"; }

MutableLateRoot::lateWrite("R");
MutableLateLeft::lateWrite("L");
MutableLateRight::lateWrite("X");
MutableLateLeft::lateWrite("L2");
MutableLateRight::selfWrite("ROOT");
$right = new MutableLateRight();
$right->instanceWrite("X2");
echo MutableLateRoot::$value . ":" . MutableLateLeft::$value . ":" . MutableLateRight::$value;
"#,
        ),
        "ROOT:L2:X2"
    );
}

#[test]
fn static_property_assignment_enforces_visibility_and_declared_existence() {
    let private = run_php_expect_error(
        r#"<?php
class PrivateMutableStatic { private static $value = 1; }
PrivateMutableStatic::$value = 2;
"#,
    );
    let rendered = format!("{private:?}");
    assert!(
        rendered.contains("Cannot access private property PrivateMutableStatic::$value"),
        "{rendered:?}"
    );

    let missing = run_php_expect_error(
        r#"<?php
class MissingMutableStatic {}
MissingMutableStatic::$value = 2;
"#,
    );
    let rendered = format!("{missing:?}");
    assert!(
        rendered.contains("Access to undeclared static property MissingMutableStatic::$value"),
        "{rendered:?}"
    );
}

#[test]
fn mutable_static_property_compound_assignments_read_then_write_canonical_storage() {
    assert_eq!(
        run_php(
            r#"<?php
class CompoundStaticRoot {
    public static $number = 1;
    public static $text = "a";
    public static function update(): void {
        static::$number += 4;
        self::$text .= "b";
    }
}
class CompoundStaticChild extends CompoundStaticRoot {}

CompoundStaticChild::update();
CompoundStaticRoot::$number *= 3;
CompoundStaticChild::$text .= "c";
echo CompoundStaticRoot::$number . ":" . CompoundStaticChild::$number . ":";
echo CompoundStaticRoot::$text . ":" . CompoundStaticChild::$text;
"#,
        ),
        "15:15:abc:abc"
    );
}

#[test]
fn closures_capture_the_late_called_class_at_creation() {
    assert_eq!(
        run_php(
            r#"<?php
class ClosureRoot {
    public static function value(): string { return "R"; }
    public static function makeClosure() {
        return function(): string { return static::value(); };
    }
    public static function makeArrow() {
        return fn(): string => static::value();
    }
    public function makeInstanceClosure() {
        return function(): string { return static::value(); };
    }
}
class ClosureLeft extends ClosureRoot {
    public static function value(): string { return "L"; }
}
class ClosureRight extends ClosureRoot {
    public static function value(): string { return "X"; }
}

$root = ClosureRoot::makeClosure();
$left = ClosureLeft::makeClosure();
$arrow = ClosureLeft::makeArrow();
$rightObject = new ClosureRight();
$instance = $rightObject->makeInstanceClosure();
echo $root();
echo $left();
echo $root();
echo $arrow();
echo $instance();
"#
        ),
        "RLRLX"
    );
}

#[test]
fn late_static_scope_preserves_compact_heap_cleanup() {
    assert_eq!(
        run_php(
            r#"<?php
class CompactLateRoot {
    public static function value(): string { return "root"; }
    public static function dispatch(): string {
        $prefix = "value:";
        $parts = [$prefix, static::value()];
        return $parts[0] . $parts[1];
    }
}
class CompactLateChild extends CompactLateRoot {
    public static function value(): string { return "child"; }
}
echo CompactLateRoot::dispatch() . ":" . CompactLateChild::dispatch();
"#
        ),
        "value:root:value:child"
    );
}

#[test]
fn wide_late_static_frame_uses_the_sparse_scope_fallback() {
    assert_eq!(
        run_php(
            r#"<?php
class WideLateRoot {
    public static function value(): string { return "root"; }
    public static function dispatch(): string {
        $v00 = 0; $v01 = 1; $v02 = 2; $v03 = 3; $v04 = 4;
        $v05 = 5; $v06 = 6; $v07 = 7; $v08 = 8; $v09 = 9;
        $v10 = 10; $v11 = 11; $v12 = 12; $v13 = 13; $v14 = 14;
        $v15 = 15; $v16 = 16; $v17 = 17; $v18 = 18; $v19 = 19;
        $v20 = 20; $v21 = 21; $v22 = 22; $v23 = 23; $v24 = 24;
        $v25 = 25; $v26 = 26; $v27 = 27; $v28 = 28; $v29 = 29;
        $v30 = 30; $v31 = 31; $v32 = 32; $v33 = 33;
        return static::value() . ($v00 + $v33);
    }
}
class WideLateChild extends WideLateRoot {
    public static function value(): string { return "child"; }
}
echo WideLateRoot::dispatch() . ":";
echo WideLateChild::dispatch() . ":";
echo WideLateRoot::dispatch();
"#
        ),
        "root33:child33:root33"
    );
}

#[test]
fn test_extends_inherits_property_default() {
    assert_eq!(
        run_php(
            r#"<?php
class Base {
    public $x = 42;
}
class Child extends Base {}
$c = new Child();
echo $c->x;
"#
        ),
        "42"
    );
}

#[test]
fn test_extends_child_overrides_method() {
    assert_eq!(
        run_php(
            r#"<?php
class Base {
    public function value() {
        return "base";
    }
}
class Child extends Base {
    public function value() {
        return "child";
    }
}
$b = new Base();
$c = new Child();
echo $b->value() . " " . $c->value();
"#
        ),
        "base child"
    );
}

#[test]
fn concrete_parent_methods_enforce_variance_without_recursive_linking() {
    let incompatible = run_php_expect_error(
        r#"<?php
interface PublishedResult {}
class UnrelatedResult {}
class PublishedFactory {
    public function create(): PublishedResult {}
}
class BrokenFactory extends PublishedFactory {
    public function create(): UnrelatedResult {}
}
"#,
    );
    assert_eq!(
        format!("{incompatible:?}"),
        "Fatal(\"Declaration of BrokenFactory::create(): UnrelatedResult must be compatible with PublishedFactory::create(): PublishedResult\")"
    );

    let recursive = run_php_expect_error(
        r#"<?php
interface StableResult {}
spl_autoload_register(function (string $requested): void {
    class DeferredFactory {
        public function make(): StableResult {}
    }
    class DeferredOverride extends DeferredFactory {
        public function make(): RecursiveResult {}
    }
});
class RecursiveResult extends MissingDependency implements RecursiveResult {}
"#,
    );
    assert_eq!(
        format!("{recursive:?}"),
        "Fatal(\"Declaration of DeferredOverride::make(): RecursiveResult must be compatible with DeferredFactory::make(): StableResult\")"
    );
}

#[test]
fn concrete_parent_variance_allows_covariance_contravariance_and_private_reuse() {
    assert_eq!(
        run_php(
            r#"<?php
class InputBase {}
class InputSpecific extends InputBase {}
class ResultBase {}
class ResultSpecific extends ResultBase {}

class ParentProcessor {
    public function process(InputSpecific $input): ResultBase { return new ResultBase(); }
    private function local(int $value): int { return $value; }
}
class ChildProcessor extends ParentProcessor {
    public function process(InputBase $input): ResultSpecific { return new ResultSpecific(); }
    public function local(string $value): string { return $value; }
}

$processor = new ChildProcessor();
echo get_class($processor->process(new InputSpecific())), '|', $processor->local('private');
"#,
        ),
        "ResultSpecific|private"
    );
}

#[test]
fn inherited_variance_resolves_iterable_trait_self_and_pending_parent_scope() {
    assert_eq!(
        run_php(
            r#"<?php
trait RequiresReplica {
    abstract public function replica(): self;
}
trait SuppliesReplica {
    public function replica(): self { return $this; }
}
class ReplicaHost {
    use RequiresReplica;
    use SuppliesReplica;
}

class IterableContract {
    public function source(): array|object {}
    public function consume(iterable $values) {}
}
class IterableImplementation extends IterableContract {
    public function source(): iterable {}
    public function consume(array|object $values) {}
}

class LexicalRoot {
    public function copy(): self { return new self(); }
}
class LexicalMiddle extends LexicalRoot {}
class LexicalLeaf extends LexicalMiddle {
    public function copy(): parent { return new LexicalMiddle(); }
}

echo get_class((new ReplicaHost())->replica()), '|', get_class((new LexicalLeaf())->copy());
"#
        ),
        "ReplicaHost|LexicalMiddle"
    );
}

#[test]
fn a_variadic_override_subsumes_fixed_typed_and_reference_parameters() {
    assert_eq!(
        run_php(
            r#"<?php
class FixedCollector {
    public function collect(int $number, string $label) {}
    public function update(&$left, &$right) {}
}
class FlexibleCollector extends FixedCollector {
    public function collect(int|string ...$items) { return count($items); }
    public function update(&...$items) { return count($items); }
}

$collector = new FlexibleCollector();
$left = 'original';
$right = 'kept';
echo $collector->collect(7, 'seven'), '|', $collector->update($left, $right);
"#
        ),
        "2|2"
    );

    let narrowed_tail = run_php_expect_error(
        r#"<?php
class OpenInputContract {
    public function ingest(int|string ...$items) {}
}
class NarrowFirstInput extends OpenInputContract {
    public function ingest(int $first = 0, int|string ...$rest) {}
}
"#,
    );
    assert_eq!(
        format!("{narrowed_tail:?}"),
        "Fatal(\"Declaration of NarrowFirstInput::ingest(int $first, int|string ...$rest) must be compatible with OpenInputContract::ingest(int|string ...$items)\")"
    );
}

#[test]
fn test_extends_child_adds_property() {
    assert_eq!(
        run_php(
            r#"<?php
class Base {
    public $x = 1;
}
class Child extends Base {
    public $y = 2;
}
$c = new Child();
echo $c->x . " " . $c->y;
"#
        ),
        "1 2"
    );
}

#[test]
fn test_extends_constructor_inherited() {
    assert_eq!(
        run_php(
            r#"<?php
class Base {
    public $name;
    public function __construct($name) {
        $this->name = $name;
    }
}
class Child extends Base {}
$c = new Child("test");
echo $c->name;
"#
        ),
        "test"
    );
}

#[test]
fn test_extends_constructor_overridden() {
    assert_eq!(
        run_php(
            r#"<?php
class Base {
    public $x;
    public function __construct($x) {
        $this->x = $x;
    }
}
class Child extends Base {
    public $y;
    public function __construct($x, $y) {
        $this->x = $x;
        $this->y = $y;
    }
}
$c = new Child(1, 2);
echo $c->x . " " . $c->y;
"#
        ),
        "1 2"
    );
}

#[test]
fn test_extends_three_levels() {
    assert_eq!(
        run_php(
            r#"<?php
class A {
    public function who() { return "A"; }
}
class B extends A {}
class C extends B {
    public function who() { return "C"; }
}
$a = new A();
$b = new B();
$c = new C();
echo $a->who() . $b->who() . $c->who();
"#
        ),
        "AAC"
    );
}

#[test]
fn test_extends_method_uses_this() {
    assert_eq!(
        run_php(
            r#"<?php
class Base {
    public $name;
    public function greet() {
        return "Hi " . $this->name;
    }
}
class Child extends Base {
    public function __construct($name) {
        $this->name = $name;
    }
}
$c = new Child("PHP");
echo $c->greet();
"#
        ),
        "Hi PHP"
    );
}

#[test]
fn test_extends_grandchild_inherits_grandparent_method() {
    // Regression: transitive inheritance must work across 3+ levels
    assert_eq!(
        run_php(
            r#"<?php
class A {
    public function foo() { return "A"; }
}
class B extends A {}
class C extends B {}
$c = new C();
echo $c->foo();
"#
        ),
        "A"
    );
}

#[test]
fn test_extends_grandchild_inherits_constructor() {
    // Regression: constructor must be inherited transitively
    assert_eq!(
        run_php(
            r#"<?php
class A {
    public $x;
    public function __construct($x) {
        $this->x = $x;
    }
}
class B extends A {}
class C extends B {}
$c = new C(42);
echo $c->x;
"#
        ),
        "42"
    );
}

#[test]
fn private_abstract_trait_methods_bind_to_the_consuming_class() {
    assert_eq!(
        run_php(
            r#"<?php
trait RequiresLocalCopy {
    abstract private function copy(self $value): self;
}
class Box {
    use RequiresLocalCopy;
    private function copy(self $value): self { return $value; }
    public function duplicate(): self { return $this->copy($this); }
}

trait RequiresTransform {
    private abstract function transform(int $value): int;
}
abstract class TransformBase {
    use RequiresTransform;
    abstract protected function transform(int $value): int;
    public function apply(int $value): int { return $this->transform($value); }
}
class Doubler extends TransformBase {
    protected function transform(int $value): int { return $value * 2; }
}

echo get_class((new Box())->duplicate()), '|', (new Doubler())->apply(21);
"#,
        ),
        "Box|42"
    );
}

#[test]
fn private_abstract_trait_methods_enforce_link_contracts() {
    let concrete_missing = run_php_expect_error(
        r#"<?php
trait RequiresConcreteHidden { abstract private function hidden(): void; }
class ConcreteIncomplete { use RequiresConcreteHidden; }
"#,
    );
    assert_eq!(
        format!("{concrete_missing:?}"),
        "Fatal(\"Class ConcreteIncomplete contains 1 abstract method and must therefore be declared abstract or implement the remaining method (ConcreteIncomplete::hidden)\")"
    );

    let missing = run_php_expect_error(
        r#"<?php
trait RequiresHidden { abstract private function hidden(): void; }
abstract class Incomplete { use RequiresHidden; }
"#,
    );
    assert_eq!(
        format!("{missing:?}"),
        "Fatal(\"Class Incomplete must implement 1 abstract method (Incomplete::hidden)\")"
    );

    let incompatible = run_php_expect_error(
        r#"<?php
trait RequiresValue { abstract private function accept(int $value): void; }
class BrokenValue {
    use RequiresValue;
    private function accept(array $value): void {}
}
"#,
    );
    assert_eq!(
        format!("{incompatible:?}"),
        "Fatal(\"Declaration of BrokenValue::accept(array $value): void must be compatible with RequiresValue::accept(int $value): void\")"
    );

    let inherited = run_php_expect_error(
        r#"<?php
class ParentContract { protected function common(): void {} }
trait RequiresArgument { private abstract function common(int $value): void; }
class BrokenChild extends ParentContract { use RequiresArgument; }
"#,
    );
    assert_eq!(
        format!("{inherited:?}"),
        "Fatal(\"Declaration of ParentContract::common(): void must be compatible with RequiresArgument::common(int $value): void\")"
    );
}

#[test]
fn nullable_method_return_variance_compares_the_inner_class_types() {
    assert_eq!(
        run_php(
            r#"<?php
interface ResultContract {}
interface KnownResult extends ResultContract {}
trait RequiresKnownResult {
    abstract public function known(): ?ResultContract;
}
class KnownImplementation {
    public function known(): ?KnownResult { return null; }
}
class KnownConsumer extends KnownImplementation { use RequiresKnownResult; }
echo (new KnownConsumer())->known() === null ? 'known' : 'bad';
"#,
        ),
        "known"
    );
}
