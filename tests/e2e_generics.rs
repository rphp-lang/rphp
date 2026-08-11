mod common;

#[cfg(any(feature = "php-generics-erased", feature = "php-generics-reified"))]
use rphp::compiler::compile::Compiler;
#[cfg(any(feature = "php-generics-erased", feature = "php-generics-reified"))]
use rphp::generics::{GenericDeclarationKind, GenericType, GenericVariance};
use rphp::lexer::Lexer;
use rphp::parser::Parser;
#[cfg(any(feature = "php-generics-erased", feature = "php-generics-reified"))]
use rphp::vm::instruction::OpType;
#[cfg(any(feature = "php-generics-erased", feature = "php-generics-reified"))]
use rphp::vm::opcode::OpCode;

fn parse(source: &str) -> Result<Vec<rphp::parser::Stmt>, String> {
    let tokens = Lexer::new(source).tokenize()?;
    Parser::new(tokens).parse()
}

#[cfg(not(any(feature = "php-generics-erased", feature = "php-generics-reified")))]
#[test]
fn default_build_contains_engine_but_rejects_generic_syntax() {
    let error = parse("<?php function id<T>(T $value): T { return $value; }").unwrap_err();
    assert_eq!(
        error,
        "Generic syntax requires php-generics-erased or php-generics-reified"
    );

    let type_error =
        parse("<?php function read(Box<int> $value): Box<int> { return $value; }").unwrap_err();
    assert_eq!(
        type_error,
        "Generic syntax requires php-generics-erased or php-generics-reified"
    );

    let static_type_error =
        parse("<?php class Box { public function copy(): static<int> {} }").unwrap_err();
    assert_eq!(
        static_type_error,
        "Generic syntax requires php-generics-erased or php-generics-reified"
    );

    let use_error = parse("<?php id::<int>(1);").unwrap_err();
    assert_eq!(
        use_error,
        "Generic syntax requires php-generics-erased or php-generics-reified"
    );

    let pseudo_static_use_error = parse(
        "<?php class C { static function id($v) { return $v; } static function call() { return self::id::<int>(1); } }",
    )
    .unwrap_err();
    assert_eq!(
        pseudo_static_use_error,
        "Generic syntax requires php-generics-erased or php-generics-reified"
    );

    let late_static_use_error = parse(
        "<?php class C { static function id($v) { return $v; } static function call() { return static::id::<int>(1); } }",
    )
    .unwrap_err();
    assert_eq!(
        late_static_use_error,
        "Generic syntax requires php-generics-erased or php-generics-reified"
    );
    parse(
        "<?php class C { static function id($v) { return $v; } static function call() { return static::id(1); } }",
    )
    .unwrap();

    let inheritance_error =
        parse("<?php class Base {} class Child extends Base<int> {}").unwrap_err();
    assert_eq!(
        inheritance_error,
        "Generic syntax requires php-generics-erased or php-generics-reified"
    );
}

#[cfg(not(any(feature = "php-generics-erased", feature = "php-generics-reified")))]
#[test]
fn reflection_layer_is_present_when_generic_syntax_is_disabled() {
    let output = common::run_php(
        r#"<?php
$reflection = new ReflectionFunction("strlen");
echo $reflection->isGeneric() ? "yes" : "no";
echo count($reflection->getGenericParameters());
echo count($reflection->getGenericRuntimeModes());
$closureReflection = new ReflectionFunction(function() {});
echo $closureReflection->isGeneric() ? "yes" : "no";
echo count($closureReflection->getGenericParameters());
"#,
    );
    assert_eq!(output, "no00no0");
}

#[cfg(any(feature = "php-generics-erased", feature = "php-generics-reified"))]
#[test]
fn ordinary_calls_execute_erased_function_method_closure_and_arrow_bounds() {
    let output = common::run_php(
        r#"<?php
function id<T>(T $value): T { return $value; }
function forwarded<U : T, T : int>(U $value): T { return $value; }
class Box<T : int> {
    public function get(T $value): T { return $value; }
    public function map<U : string>(U $value): U { return $value; }
}
$closure = function<C : int>(C $value): C { return $value; };
$arrow = fn<A : string>(A $value): A => $value;
$box = new Box();
echo id(2);
echo forwarded(3);
echo $box->get(4);
echo $box->map("m");
echo $closure(5);
echo $arrow("a");
"#,
    );
    assert_eq!(output, "234m5a");
}

#[cfg(any(feature = "php-generics-erased", feature = "php-generics-reified"))]
#[test]
fn erased_bound_uses_existing_runtime_type_check() {
    let error = common::run_php_expect_error(
        "<?php function only_int<T : int>(T $value): T { return $value; } only_int(\"bad\");",
    );
    assert!(format!("{error:?}").contains("must be of type int"));
}

#[cfg(any(feature = "php-generics-erased", feature = "php-generics-reified"))]
#[test]
fn explicit_type_arguments_cover_function_class_method_static_and_dynamic_calls() {
    let output = common::run_php(
        r#"<?php
function id<T : int>(T $value): T { return $value; }
class Box<T : object> {
    public function map<U : string>(U $value): U { return $value; }
    public static function twice<V : int>(V $value): V { return $value; }
}
$box = new Box::<stdClass>();
echo id::<int>(1);
echo id::<int>(2);
echo $box->map::<string>("m");
echo Box::twice::<int>(3);
$callable = "id";
echo ($callable)::<int>(4);
$closure = function<C : int>(C $value): C { return $value; };
echo $closure::<int>(5);
"#,
    );
    assert_eq!(output, "12m345");
}

#[cfg(any(feature = "php-generics-erased", feature = "php-generics-reified"))]
#[test]
fn explicit_type_arguments_cover_nullsafe_self_and_parent_calls() {
    let output = common::run_php(
        r#"<?php
namespace GenericCalls;

class Base {
    public function instance<T : int>(T $value): T { return $value; }
    public static function inherited<T : int>(T $value): T { return $value; }
    public static function factory<T>(): static { return new Child(); }
}

class Child extends Base {
    public static function own<T : string>(T $value): T { return $value; }
    public static function calls(): string {
        return self::own::<string>("s")
            . self::inherited::<int>(2)
            . parent::inherited::<int>(3);
    }
    public static function selfFactory(): static {
        return self::factory::<int>();
    }
    public static function parentFactory(): static {
        return parent::factory::<int>();
    }
}

$missing = null;
$present = new Child();
echo ($missing?->instance::<int>(4) ?? "n") . ":";
echo $present?->instance::<int>(5) . ":";
echo Child::calls() . ":";
echo Child::selfFactory() instanceof Child ? "self:" : "bad:";
echo Child::parentFactory() instanceof Child ? "parent" : "bad";
"#,
    );
    assert_eq!(output, "n:5:s23:self:parent");
}

#[cfg(any(feature = "php-generics-erased", feature = "php-generics-reified"))]
#[test]
fn trait_self_turbofish_resolves_each_consuming_class() {
    let output = common::run_php(
        r#"<?php
trait CallsGenericScope {
    public static function call(): string {
        return self::value::<string>();
    }
}
class First {
    use CallsGenericScope;
    public static function value<T : string>(): string { return "first"; }
}
class Second {
    use CallsGenericScope;
    public static function value<T : string>(): string { return "second"; }
}
echo First::call() . ":" . Second::call() . ":" . First::call();
"#,
    );
    assert_eq!(output, "first:second:first");
}

#[cfg(any(feature = "php-generics-erased", feature = "php-generics-reified"))]
#[test]
fn late_static_turbofish_rekeys_declaration_and_call_caches() {
    let output = common::run_php(
        r#"<?php
class LateGenericRoot {
    public static function value<T : string>(): string { return "root"; }
    public static function dispatch(): string { return static::value::<string>(); }
}
class LateGenericFirst extends LateGenericRoot {
    public static function value<T : string>(): string { return "first"; }
}
class LateGenericSecond extends LateGenericRoot {
    public static function value<T : string>(): string { return "second"; }
}
echo LateGenericRoot::dispatch() . ":";
echo LateGenericFirst::dispatch() . ":";
echo LateGenericSecond::dispatch() . ":";
echo LateGenericFirst::dispatch();
"#,
    );
    assert_eq!(output, "root:first:second:first");
}

#[cfg(any(feature = "php-generics-erased", feature = "php-generics-reified"))]
#[test]
fn late_static_turbofish_initializer_tracks_its_adjacent_guard_on_x86_64() {
    let statements = parse(
        r#"<?php
class LateGenericOpcodes {
    public static function value<T : string>(): string { return "value"; }
    public static function checked(): string { return static::value::<string>(); }
    public static function ordinary(): string { return static::value(); }
}
"#,
    )
    .unwrap();
    let result = Compiler::new().compile(&statements).unwrap();
    let class = &result.class_defs[0];
    let checked = &class
        .methods
        .iter()
        .find(|(name, ..)| name == "checked")
        .unwrap()
        .4
        .op_array
        .instructions;
    let init_ip = checked
        .iter()
        .position(|instruction| instruction.opcode == OpCode::InitLateStaticCall)
        .unwrap();
    assert!(init_ip > 0);
    assert_eq!(
        checked[init_ip - 1].opcode,
        OpCode::CheckLateStaticGenericArgs
    );

    #[cfg(target_arch = "x86_64")]
    {
        assert_eq!(checked[init_ip].result_type, OpType::Const);
        assert_eq!(checked[init_ip].result as usize, init_ip - 1);
    }
    #[cfg(not(target_arch = "x86_64"))]
    assert_eq!(checked[init_ip].result_type, OpType::Unused);

    let ordinary = &class
        .methods
        .iter()
        .find(|(name, ..)| name == "ordinary")
        .unwrap()
        .4
        .op_array
        .instructions;
    let ordinary_init = ordinary
        .iter()
        .find(|instruction| instruction.opcode == OpCode::InitLateStaticCall)
        .unwrap();
    assert_eq!(ordinary_init.result_type, OpType::Unused);
}

#[cfg(any(feature = "php-generics-erased", feature = "php-generics-reified"))]
#[test]
fn closure_late_static_turbofish_keeps_its_creation_scope() {
    let output = common::run_php(
        r#"<?php
class ClosureGenericRoot {
    public static function value<T : string>(): string { return "root"; }
    public static function make() {
        return function(): string { return static::value::<string>(); };
    }
}
class ClosureGenericChild extends ClosureGenericRoot {
    public static function value<T : string>(): string { return "child"; }
}
$root = ClosureGenericRoot::make();
$child = ClosureGenericChild::make();
echo $root() . ":" . $child() . ":" . $root();
"#,
    );
    assert_eq!(output, "root:child:root");
}

#[cfg(any(feature = "php-generics-erased", feature = "php-generics-reified"))]
#[test]
fn late_static_turbofish_validates_the_selected_override() {
    let error = common::run_php_expect_error(
        r#"<?php
trait LateBoundDispatch {
    public static function dispatch() { return static::value::<int>(1); }
}
class LateBoundInt {
    use LateBoundDispatch;
    public static function value<T : int>(T $value): T { return $value; }
}
class LateBoundString {
    use LateBoundDispatch;
    public static function value<T : string>(T $value): T { return $value; }
}
LateBoundInt::dispatch();
LateBoundString::dispatch();
"#,
    );
    let rendered = format!("{error:?}");
    assert!(rendered.contains("does not satisfy bound"), "{rendered:?}");
}

#[cfg(all(feature = "php-generics-erased", not(feature = "php-generics-reified")))]
#[test]
fn erased_late_static_turbofish_keeps_bound_erased_runtime_contract() {
    let output = common::run_php(
        r#"<?php
class LateErased {
    public static function value<T>(T $value): T { return $value; }
    public static function dispatch() { return static::value::<int>("erased"); }
}
echo LateErased::dispatch();
"#,
    );
    assert_eq!(output, "erased");
}

#[cfg(feature = "php-generics-reified")]
#[test]
fn reified_late_static_turbofish_checks_the_explicit_runtime_contract() {
    let error = common::run_php_expect_error(
        r#"<?php
class LateReified {
    public static function value<T>(T $value): T { return $value; }
    public static function dispatch() { return static::value::<int>("bad"); }
}
LateReified::dispatch();
"#,
    );
    let rendered = format!("{error:?}");
    assert!(
        rendered.contains("does not match its reified generic type"),
        "{rendered:?}"
    );
}

#[cfg(any(feature = "php-generics-erased", feature = "php-generics-reified"))]
#[test]
fn trait_late_static_turbofish_follows_each_consuming_class() {
    let output = common::run_php(
        r#"<?php
trait LateGenericTrait {
    public static function dispatch(): string {
        return static::value::<string>();
    }
}
class LateGenericTraitFirst {
    use LateGenericTrait;
    public static function value<T : string>(): string { return "first"; }
}
class LateGenericTraitSecond {
    use LateGenericTrait;
    public static function value<T : string>(): string { return "second"; }
}
echo LateGenericTraitFirst::dispatch() . ":";
echo LateGenericTraitSecond::dispatch() . ":";
echo LateGenericTraitFirst::dispatch();
"#,
    );
    assert_eq!(output, "first:second:first");
}

#[cfg(any(feature = "php-generics-erased", feature = "php-generics-reified"))]
#[test]
fn self_and_parent_turbofish_validate_the_selected_declaration() {
    for source in [
        r#"<?php
class Scoped {
    public static function value<T : int>(T $value): T { return $value; }
    public static function invalid() { return self::value::<string>(1); }
}
Scoped::invalid();
"#,
        r#"<?php
class ScopedBase {
    public static function value<T : int>(T $value): T { return $value; }
}
class ScopedChild extends ScopedBase {
    public static function invalid() { return parent::value::<string>(1); }
}
ScopedChild::invalid();
"#,
    ] {
        let error = common::run_php_expect_error(source);
        let rendered = format!("{error:?}");
        assert!(rendered.contains("does not satisfy bound"), "{rendered:?}");
    }
}

#[cfg(all(feature = "php-generics-erased", not(feature = "php-generics-reified")))]
#[test]
fn erased_self_turbofish_keeps_bound_erased_runtime_contract() {
    let output = common::run_php(
        r#"<?php
class Scoped {
    public static function value<T>(T $value): T { return $value; }
    public static function call() { return self::value::<int>("erased"); }
}
echo Scoped::call();
"#,
    );
    assert_eq!(output, "erased");
}

#[cfg(feature = "php-generics-reified")]
#[test]
fn reified_self_turbofish_checks_the_explicit_runtime_contract() {
    let error = common::run_php_expect_error(
        r#"<?php
class Scoped {
    public static function value<T>(T $value): T { return $value; }
    public static function call() { return self::value::<int>("bad"); }
}
Scoped::call();
"#,
    );
    let rendered = format!("{error:?}");
    assert!(
        rendered.contains("does not match its reified generic type"),
        "{rendered:?}"
    );
}

#[cfg(any(feature = "php-generics-erased", feature = "php-generics-reified"))]
#[test]
fn generic_method_arguments_follow_the_selected_method_body() {
    let output = common::run_php(
        r#"<?php
trait GenericMethods {
    public function value<T>(T $value): T { return $value; }
    public static function staticValue<U>(U $value): U { return $value; }
}
class ImportsGenericMethods { use GenericMethods; }
class GenericParent {
    public function inherited<T>(T $value): T { return $value; }
}
class GenericChild extends GenericParent {}
$object = new ImportsGenericMethods();
echo $object->value::<int>(1);
echo ImportsGenericMethods::staticValue::<string>("s");
echo (new GenericChild())->inherited::<int>(2);
"#,
    );
    assert_eq!(output, "1s2");
}

#[cfg(any(feature = "php-generics-erased", feature = "php-generics-reified"))]
#[test]
fn generic_pseudo_type_applications_keep_their_lexical_class_scope() {
    let output = common::run_php(
        r#"<?php
class ScopedParent<T> {}
class ScopedChild<U> extends ScopedParent<U> {
    public self<U> $peer;
    public function same(self<U> $value): self<U> { return $value; }
    public function ancestor(parent<U> $value): parent<U> { return $value; }
}
$child = new ScopedChild::<int>();
$child->peer = $child;
echo $child->same($child) instanceof ScopedChild ? "self:" : "bad:";
echo $child->ancestor(new ScopedParent::<int>()) instanceof ScopedParent ? "parent:" : "bad:";
echo $child->peer instanceof ScopedChild ? "property:" : "bad:";

class InheritedParent<T> {
    public self<T> $peer;
    public function same(self<T> $value): self<T> { return $value; }
}
class InheritedChild<U> extends InheritedParent<U> {}
$inherited = new InheritedChild::<int>();
$parent = new InheritedParent::<int>();
$inherited->peer = $parent;
echo $inherited->same($parent) instanceof InheritedParent ? "inherited:" : "bad:";

trait ScopedTrait<T> {
    public self<T> $peer;
    public function same(self<T> $value): self<T> { return $value; }
}
class TraitBase { use ScopedTrait<int>; }
class TraitChild extends TraitBase {}
$traitChild = new TraitChild();
$traitBase = new TraitBase();
$traitChild->peer = $traitBase;
echo $traitChild->same($traitBase) instanceof TraitBase ? "trait:" : "bad:";

trait ScopedMethodTrait {
    public function sameGeneric<V>(self<V> $value): self<V> { return $value; }
}
class MethodTraitBase<X> { use ScopedMethodTrait; }
class MethodTraitChild<Y> extends MethodTraitBase<Y> {}
$methodTraitChild = new MethodTraitChild::<int>();
$methodTraitBase = new MethodTraitBase::<int>();
echo $methodTraitChild->sameGeneric::<int>($methodTraitBase) instanceof MethodTraitBase
    ? "method-trait:"
    : "bad";

class BoundParent {}
class BoundScope extends BoundParent {
    public function sameBound<V : self>(V $value): V { return $value; }
    public function parentBound<V : parent>(V $value): V { return $value; }
}
$boundScope = new BoundScope();
echo $boundScope->sameBound::<BoundScope>($boundScope) instanceof BoundScope
    && $boundScope->parentBound::<BoundParent>(new BoundParent()) instanceof BoundParent
    ? "bounds:"
    : "bad:";

trait BoundTrait {
    public function traitBound<V : self>(V $value): V { return $value; }
}
class BoundTraitBase { use BoundTrait; }
class BoundTraitChild extends BoundTraitBase {}
$boundTraitChild = new BoundTraitChild();
$boundTraitBase = new BoundTraitBase();
echo $boundTraitChild->traitBound::<BoundTraitBase>($boundTraitBase) instanceof BoundTraitBase
    ? "trait-bound"
    : "bad";
"#,
    );
    assert_eq!(
        output,
        "self:parent:property:inherited:trait:method-trait:bounds:trait-bound"
    );
}

#[cfg(any(feature = "php-generics-erased", feature = "php-generics-reified"))]
#[test]
fn generic_static_application_uses_the_late_called_class() {
    let output = common::run_php(
        r#"<?php
class StaticGenericBase<T> {
    public function copy(): static<T> { return $this; }
    public function wrongClass(): static<T> { return new StaticGenericBase::<int>(); }
    public static function factory<U>(): static<U> {
        return new StaticGenericChild::<int>();
    }
    public static function wrongFactory<U>(): static<U> {
        return new StaticGenericBase::<int>();
    }
}
class StaticGenericChild<V> extends StaticGenericBase<V> {}

$value = new StaticGenericChild::<int>();
echo $value->copy() instanceof StaticGenericChild ? "instance:" : "bad:";
echo StaticGenericChild::factory::<int>() instanceof StaticGenericChild ? "static" : "bad";

trait StaticGenericTrait<T> {
    public function traitCopy(): static<T> { return $this; }
}
class StaticTraitBase<U> { use StaticGenericTrait<U>; }
class StaticTraitChild<V> extends StaticTraitBase<V> {}
$trait = new StaticTraitChild::<int>();
echo $trait->traitCopy() instanceof StaticTraitChild ? ":trait" : ":bad";
"#,
    );
    assert_eq!(output, "instance:static:trait");

    for source in [
        r#"<?php
class StaticGenericBase<T> {
    public function wrongClass(): static<T> { return new StaticGenericBase::<int>(); }
}
class StaticGenericChild<V> extends StaticGenericBase<V> {}
$value = new StaticGenericChild::<int>();
$value->wrongClass();
"#,
        r#"<?php
class StaticGenericBase<T> {
    public static function wrongFactory<U>(): static<U> {
        return new StaticGenericBase::<int>();
    }
}
class StaticGenericChild<V> extends StaticGenericBase<V> {}
StaticGenericChild::wrongFactory::<int>();
"#,
    ] {
        let error = common::run_php_expect_error(source);
        let rendered = format!("{error:?}");
        assert!(rendered.contains("Return value"), "{rendered:?}");
    }
}

#[cfg(any(feature = "php-generics-erased", feature = "php-generics-reified"))]
#[test]
fn generic_static_application_is_not_namespace_resolved() {
    let output = common::run_php(
        r#"<?php
namespace StaticScope;
class Base<T> {
    public function copy(): static<T> { return $this; }
}
class Child<U> extends Base<U> {}
$value = new Child::<int>();
echo $value->copy() instanceof Child ? "yes" : "no";
"#,
    );
    assert_eq!(output, "yes");
}

#[cfg(feature = "php-generics-reified")]
#[test]
fn reified_static_application_checks_called_class_arguments() {
    let error = common::run_php_expect_error(
        r#"<?php
class ReifiedStaticBase<T> {
    public function wrongArguments(): static<T> {
        return new ReifiedStaticChild::<string>();
    }
}
class ReifiedStaticChild<U> extends ReifiedStaticBase<U> {}
$value = new ReifiedStaticChild::<int>();
$value->wrongArguments();
"#,
    );
    let rendered = format!("{error:?}");
    assert!(rendered.contains("reified"), "{rendered:?}");

    let static_error = common::run_php_expect_error(
        r#"<?php
class ReifiedStaticFactory<T> {
    public static function wrongArguments<U>(): static<U> {
        return new ReifiedStaticFactoryChild::<string>();
    }
}
class ReifiedStaticFactoryChild<V> extends ReifiedStaticFactory<V> {}
ReifiedStaticFactoryChild::wrongArguments::<int>();
"#,
    );
    let rendered = format!("{static_error:?}");
    assert!(rendered.contains("reified"), "{rendered:?}");
}

#[cfg(feature = "php-generics-reified")]
#[test]
fn imported_generic_method_keeps_its_reified_contract() {
    let error = common::run_php_expect_error(
        "<?php trait T { public function id<U>(U $v): U { return $v; } } class C { use T; } $c = new C(); $c->id::<int>(\"bad\");",
    );
    let rendered = format!("{error:?}");
    assert!(
        rendered.contains("does not match its reified generic type"),
        "{rendered:?}"
    );
}

#[cfg(any(feature = "php-generics-erased", feature = "php-generics-reified"))]
#[test]
fn explicit_type_arguments_validate_arity_defaults_and_bounds() {
    let output = common::run_php(
        r#"<?php
class Base {}
class Child extends Base {}
function make<T : Base, U : int = int>(U $value): U { return $value; }
echo make::<Child>(7);
"#,
    );
    assert_eq!(output, "7");

    for (source, expected) in [
        (
            "<?php function id<T : int>(T $v): T { return $v; } id::<string>(1);",
            "does not satisfy bound",
        ),
        (
            "<?php function pair<T, U>(T $a, U $b): T { return $a; } pair::<int>(1, 2);",
            "expects 2 to 2 type arguments, 1 given",
        ),
        (
            "<?php function id<T>(T $v): T { return $v; } id::<int, string>(1);",
            "expects 1 to 1 type arguments, 2 given",
        ),
        (
            "<?php function plain($v) { return $v; } plain::<int>(1);",
            "non-generic function plain",
        ),
    ] {
        let error = common::run_php_expect_error(source);
        let rendered = format!("{error:?}");
        assert!(
            rendered.contains(expected),
            "{rendered:?} did not contain {expected:?}"
        );
    }
}

#[cfg(any(feature = "php-generics-erased", feature = "php-generics-reified"))]
#[test]
fn inheritance_arguments_link_with_arity_bounds_and_forwarding() {
    let output = common::run_php(
        r#"<?php
class Base {}
class Concrete extends Base {}
class ParentBox<T : Base> {}
class Forwarded<U : Concrete> extends ParentBox<U> {}
class DefaultParent<T : Base = Concrete> {}
class UsesDefault extends DefaultParent {}
interface Sink<T : int> {}
trait Carries<T : int> { public T $value; }
class Combined<U : int> implements Sink<U> {
    use Carries<U>;
}
$value = new Forwarded::<Concrete>();
$combined = new Combined::<int>();
$combined->value = 7;
echo ($value instanceof ParentBox) ? "parent:" : "missing:";
echo $combined->value;
echo (new UsesDefault()) instanceof DefaultParent ? ":default" : ":missing";
"#,
    );
    assert_eq!(output, "parent:7:default");

    for (source, expected) in [
        (
            "<?php class ParentBox<T> {} class Missing extends ParentBox {}",
            "expects 1 to 1 type arguments, 0 given",
        ),
        (
            "<?php class Base {} class ParentBox<T : Base> {} class Bad extends ParentBox<string> {}",
            "does not satisfy bound",
        ),
        (
            "<?php class Base {} class ParentBox<T : Base> {} class Bad<U> extends ParentBox<U> {}",
            "does not satisfy bound",
        ),
        (
            "<?php class Plain {} class Bad extends Plain<int> {}",
            "non-generic ancestor Plain",
        ),
    ] {
        let error = common::run_php_expect_error(source);
        let rendered = format!("{error:?}");
        assert!(
            rendered.contains(expected),
            "{rendered:?} did not contain {expected:?}"
        );
    }
}

#[cfg(any(feature = "php-generics-erased", feature = "php-generics-reified"))]
#[test]
fn inheritance_bounds_resolve_lexical_class_pseudo_types() {
    let output = common::run_php(
        r#"<?php
class ScopeRoot {}
class SelfScoped<T : self> {}
class SelfGood extends SelfScoped<SelfScoped> {}
class SelfForwarded<U : self> extends SelfScoped<U> {}
class ParentScoped<T : parent> extends ScopeRoot {}
class ParentGood extends ParentScoped<ScopeRoot> {}
class ParentForwarded<U : self> extends ParentScoped<U> {}
trait TraitSelfScoped<T : self> {}
class TraitSelfGood { use TraitSelfScoped<TraitSelfGood>; }
trait TraitParentScoped<T : parent> {}
class TraitParentGood extends ScopeRoot { use TraitParentScoped<ScopeRoot>; }
class TraitParentForwarded<U : self> extends ScopeRoot { use TraitParentScoped<U>; }
echo (new SelfGood()) instanceof SelfScoped ? "self:" : "bad:";
echo (new SelfForwarded::<SelfForwarded>()) instanceof SelfScoped ? "forward-self:" : "bad:";
echo (new ParentGood()) instanceof ParentScoped ? "parent:" : "bad:";
echo (new ParentForwarded::<ParentForwarded>()) instanceof ParentScoped ? "forward-parent:" : "bad:";
echo (new TraitSelfGood()) instanceof TraitSelfGood ? "trait-self:" : "bad:";
echo (new TraitParentGood()) instanceof ScopeRoot ? "trait-parent" : "bad";
echo (new TraitParentForwarded::<TraitParentForwarded>()) instanceof ScopeRoot ? ":trait-forward" : ":bad";
"#,
    );
    assert_eq!(
        output,
        "self:forward-self:parent:forward-parent:trait-self:trait-parent:trait-forward"
    );

    for source in [
        "<?php class ScopeRoot {} class Scoped<T : self> {} class Bad extends Scoped<ScopeRoot> {}",
        "<?php class ScopeRoot {} class Scoped<T : parent> extends ScopeRoot {} class Unrelated {} class Bad extends Scoped<Unrelated> {}",
        "<?php class ScopeRoot {} trait Scoped<T : self> {} class Bad { use Scoped<ScopeRoot>; }",
        "<?php class ScopeRoot {} trait Scoped<T : parent> {} class Bad { use Scoped<ScopeRoot>; }",
    ] {
        let error = common::run_php_expect_error(source);
        let rendered = format!("{error:?}");
        assert!(rendered.contains("does not satisfy bound"), "{rendered:?}");
    }
}

#[cfg(any(feature = "php-generics-erased", feature = "php-generics-reified"))]
#[test]
fn parametric_lsp_substitutes_direct_ancestor_method_signatures() {
    let output = common::run_php(
        r#"<?php
interface Transformer<T> { public function apply(T $value): T; }
class WideTransformer implements Transformer<int> {
    public function apply(mixed $value): int { return 1; }
}
class ParentBox<T> { public function read(): T { return null; } }
class IntBox extends ParentBox<int> { public function read(): int { return 2; } }
class ForwardedBox<U : int> extends ParentBox<U> {
    public function read(): U { return 3; }
}
trait Reader<T> { public function traitRead(): T { return null; } }
class TraitReader { use Reader<int>; public function traitRead(): int { return 4; } }
interface RootReader<T> { public function rootRead(): T; }
interface MiddleReader<U> extends RootReader<U> {}
class TransitiveReader implements MiddleReader<int> {
    public function rootRead(): int { return 5; }
}
class VariadicParent<T> { public function collect(T ...$values): T { return $values[0]; } }
class VariadicChild extends VariadicParent<int> {
    public function collect(mixed ...$values): int { return $values[0]; }
}
class ResultParent<+T> {}
class ResultChild<U> extends ResultParent<U> {}
interface ResultSource<T> { public function result(): ResultParent<T>; }
class CovariantResult implements ResultSource<int> {
    public function result(): ResultChild<int> { return null; }
}
echo (new WideTransformer())->apply("wide");
echo (new IntBox())->read();
echo (new ForwardedBox::<int>())->read();
echo (new TraitReader())->traitRead();
echo (new TransitiveReader())->rootRead();
echo (new VariadicChild())->collect(6, 7);
"#,
    );
    assert_eq!(output, "123456");

    for (source, expected) in [
        (
            "<?php interface Source<T> { public function get(): T; } class Bad implements Source<int> { public function get(): string { return 'bad'; } }",
            "return type",
        ),
        (
            "<?php interface Sink<T> { public function put(T $value); } class Bad implements Sink<int> { public function put(string $value) {} }",
            "parameter 1",
        ),
        (
            "<?php class ParentBox<T> { public function get(): T { return null; } } class Bad extends ParentBox<int> { public function get(): string { return 'bad'; } }",
            "return type",
        ),
        (
            "<?php trait Reader<T> { public function get(): T { return null; } } class Bad { use Reader<int>; public function get(): string { return 'bad'; } }",
            "return type",
        ),
        (
            "<?php interface Root<T> { public function get(): T; } interface Middle<U> extends Root<U> {} class Bad implements Middle<int> { public function get(): string { return 'bad'; } }",
            "return type",
        ),
        (
            "<?php class ParentPair<T> { public function set(T $first, T $second) {} } class Bad extends ParentPair<int> { public function set(int $first) {} }",
            "accepts 1 parameters",
        ),
        (
            "<?php class OptionalParent<T> { public function set(T $value = null) {} } class Bad extends OptionalParent<int> { public function set(int $value) {} }",
            "requires 1 parameters",
        ),
        (
            "<?php class VariadicParent<T> { public function set(T ...$values) {} } class Bad extends VariadicParent<int> { public function set(int $value) {} }",
            "must remain variadic",
        ),
        (
            "<?php class ResultParent<+T> {} class ResultChild<U> extends ResultParent<U> {} interface Source<T> { public function get(): ResultParent<T>; } class Bad implements Source<int> { public function get(): ResultChild<string> { return null; } }",
            "return type",
        ),
    ] {
        let error = common::run_php_expect_error(source);
        let rendered = format!("{error:?}");
        assert!(
            rendered.contains("Parametric LSP violation"),
            "{rendered:?}"
        );
        assert!(
            rendered.contains(expected),
            "{rendered:?} did not contain {expected:?}"
        );
    }
}

#[cfg(any(feature = "php-generics-erased", feature = "php-generics-reified"))]
#[test]
fn parametric_lsp_alpha_renames_method_generic_parameters() {
    let output = common::run_php(
        r#"<?php
interface Mapper<T> {
    public function map<U, V : T>(U $left): U;
}
class IntMapper implements Mapper<int> {
    public function map<A, B : int>(A $left): A { return $left; }
}
interface PlainMapper {
    public function choose<L, R>(L $left, R $right): R;
}
class RenamedMapper implements PlainMapper {
    public function choose<X, Y>(X $left, Y $right): Y { return $right; }
}
$intMapper = new IntMapper();
$renamedMapper = new RenamedMapper();
echo $intMapper->map::<string, int>("alpha");
echo ":" . $renamedMapper->choose::<int, string>(2, "beta");
"#,
    );
    assert_eq!(output, "alpha:beta");

    for (source, expected) in [
        (
            "<?php interface Mapper { public function map<A, B>(A $value): B; } class Bad implements Mapper { public function map<X, Y>(Y $value): X { return $value; } }",
            "parameter 1",
        ),
        (
            "<?php interface Mapper { public function map<A : int>(A $value): A; } class Bad implements Mapper { public function map<X : string>(X $value): X { return $value; } }",
            "bound of generic parameter 1",
        ),
        (
            "<?php interface Mapper { public function map<A>(A $value): A; } class Bad implements Mapper { public function map<X, Y>(X $value): X { return $value; } }",
            "declares 2 generic parameters",
        ),
        (
            "<?php interface Mapper { public function map<+A>(): A; } class Bad implements Mapper { public function map<-X>(): mixed { return null; } }",
            "incompatible variance",
        ),
        (
            "<?php interface Mapper { public function map<A : int = int>(A $value): A; } class Bad implements Mapper { public function map<X : int>(X $value): X { return $value; } }",
            "default of generic parameter 1",
        ),
    ] {
        let error = common::run_php_expect_error(source);
        let rendered = format!("{error:?}");
        assert!(
            rendered.contains("Parametric LSP violation"),
            "{rendered:?}"
        );
        assert!(
            rendered.contains(expected),
            "{rendered:?} did not contain {expected:?}"
        );
    }
}

#[cfg(any(feature = "php-generics-erased", feature = "php-generics-reified"))]
#[test]
fn generic_diamond_contracts_merge_by_use_site_polarity() {
    let output = common::run_php(
        r#"<?php
interface Renderable {}
interface Cacheable {}
class Article implements Renderable, Cacheable {}
interface Pipeline<T : object> {
    public function process(T $value): T;
}
interface RenderingPipeline extends Pipeline<Renderable> {}
interface CachingPipeline extends Pipeline<Cacheable> {}
class ArticlePipeline implements
    RenderingPipeline,
    CachingPipeline,
    Pipeline<Renderable&Cacheable>
{
    public function process(Renderable|Cacheable $value): Renderable&Cacheable {
        return new Article();
    }
}
$pipeline = new ArticlePipeline();
$result = $pipeline->process(new Article());
echo ($result instanceof Renderable) ? "renderable:" : "missing:";
echo ($result instanceof Cacheable) ? "cacheable" : "missing";
"#,
    );
    assert_eq!(output, "renderable:cacheable");

    let statements = parse(
        "<?php interface A {} interface B {} function both<T>(A&B $value): A&B { return $value; }",
    )
    .unwrap();
    let result = Compiler::new().compile(&statements).unwrap();
    let declaration = result
        .generic_metadata
        .find(GenericDeclarationKind::Function, "both")
        .expect("intersection-bearing declaration metadata");
    assert!(matches!(
        declaration.value_parameters[0],
        Some(GenericType::Intersection(_))
    ));
    assert!(matches!(
        declaration.return_type,
        Some(GenericType::Intersection(_))
    ));
}

#[cfg(any(feature = "php-generics-erased", feature = "php-generics-reified"))]
#[test]
fn reflection_exposes_parent_interface_trait_and_nested_argument_bindings() {
    let output = common::run_php(
        r#"<?php
interface Renderable {}
interface Cacheable {}
interface Foo<T> {}
class Dup implements Foo<string>, Foo<int> {}

class Box<T> {}
class IntBox extends Box<int> {}
class Forward<U> extends Box<U> {}
class Nested extends Box<Foo<int>> {}
class Compound extends Box<Renderable&Cacheable> {}

trait Holder<T> {}
class StringHolder { use Holder<string>; }

$parent = (new ReflectionClass("IntBox"))->getGenericArgumentsForParentClass();
echo get_class($parent[0]) . ":" . $parent[0]->getName() . ":";

$bindings = (new ReflectionClass("Dup"))->getGenericArgumentsForParentInterface("Foo");
echo count($bindings) . ":" . $bindings[0][0]->getName() . ":" . $bindings[1][0]->getName() . ":";

$trait = (new ReflectionClass("StringHolder"))->getGenericArgumentsForUsedTrait("Holder");
echo $trait[0]->getName() . ":";

$forward = (new ReflectionClass("Forward"))->getGenericArgumentsForParentClass();
echo get_class($forward[0]) . ":" . $forward[0]->name . ":" . $forward[0]->getName() . ":";
echo get_class($forward[0]->getTypeParameter()) . ":" . $forward[0]->getTypeParameter()->getName() . ":";

$nested = (new ReflectionClass("Nested"))->getGenericArgumentsForParentClass();
echo $nested[0]->getName() . ":";
echo $nested[0]->hasGenericArguments() ? "yes:" : "no:";
echo $nested[0]->getGenericArguments()[0]->getName() . ":";

$compound = (new ReflectionClass("Compound"))->getGenericArgumentsForParentClass();
echo get_class($compound[0]) . ":";
echo $compound[0]->getTypes()[0]->getName() . ":" . $compound[0]->getTypes()[1]->getName();

try {
    $invalid = new ReflectionClass("Dup");
    $invalid->getGenericArgumentsForParentInterface("Renderable");
} catch (ReflectionException $error) {
    echo ":caught";
}
"#,
    );
    assert_eq!(
        output,
        "ReflectionNamedType:int:2:string:int:string:ReflectionTypeParameterReference:U:U:ReflectionGenericTypeParameter:U:Foo:yes:int:ReflectionIntersectionType:Renderable:Cacheable:caught"
    );
}

#[cfg(any(feature = "php-generics-erased", feature = "php-generics-reified"))]
#[test]
fn inherited_trait_diamonds_enforce_merged_runtime_contracts() {
    let output = common::run_php(
        r#"<?php
interface Renderable {}
interface Cacheable {}
class Article implements Renderable, Cacheable {}
class RenderOnly implements Renderable {}
trait Pipeline<T : object> {
    public T $value;
    public function process(T $value): T { return new Article(); }
}
class ForwardDiamond { use Pipeline<Renderable>, Pipeline<Cacheable>; }
class ReverseDiamond { use Pipeline<Cacheable>, Pipeline<Renderable>; }
$forward = new ForwardDiamond();
$reverse = new ReverseDiamond();
$forward->value = new RenderOnly();
$reverse->value = new RenderOnly();
echo ($forward->process(new RenderOnly()) instanceof Cacheable) ? "forward:" : "missing:";
echo ($reverse->process(new RenderOnly()) instanceof Cacheable) ? "reverse" : "missing";
"#,
    );
    assert_eq!(output, "forward:reverse");

    for source in [
        "<?php interface Renderable {} interface Cacheable {} trait Pipeline<T : object> { public function process(T $value): T { return $value; } } class Diamond { use Pipeline<Renderable>, Pipeline<Cacheable>; } $value = new stdClass(); $diamond = new Diamond(); $diamond->process($value);",
        "<?php interface Renderable {} interface Cacheable {} class RenderOnly implements Renderable {} trait Pipeline<T : object> { public function process(T $value): T { return $value; } } class Diamond { use Pipeline<Renderable>, Pipeline<Cacheable>; } $value = new RenderOnly(); $diamond = new Diamond(); $diamond->process($value);",
        "<?php interface Renderable {} interface Cacheable {} trait Slot<T : object> { public T $value; } class Diamond { use Slot<Cacheable>, Slot<Renderable>; } $diamond = new Diamond(); $diamond->value = new stdClass();",
    ] {
        let error = common::run_php_expect_error(source);
        let rendered = format!("{error:?}");
        assert!(
            rendered.contains("generic") || rendered.contains("property"),
            "{rendered:?}"
        );
    }

    let statements = parse(
        r#"<?php
interface Renderable {}
interface Cacheable {}
trait Pipeline<T : object> {
    public T $value;
    public function process(T $value): T { return $value; }
}
class ForwardDiamond { use Pipeline<Renderable>, Pipeline<Cacheable>; }
class ReverseDiamond { use Pipeline<Cacheable>, Pipeline<Renderable>; }
"#,
    )
    .unwrap();
    let result = Compiler::new().compile(&statements).unwrap();
    let metadata = &result.generic_metadata;
    let forward = metadata
        .find_class_like_index("ForwardDiamond")
        .expect("forward diamond metadata");
    let reverse = metadata
        .find_class_like_index("ReverseDiamond")
        .expect("reverse diamond metadata");
    let forward_method = metadata
        .linked_instance_method_contract(forward, "process")
        .expect("forward method contract");
    let reverse_method = metadata
        .linked_instance_method_contract(reverse, "process")
        .expect("reverse method contract");
    assert_eq!(
        forward_method.value_parameters,
        reverse_method.value_parameters
    );
    assert_eq!(forward_method.return_type, reverse_method.return_type);
    assert_eq!(
        metadata.linked_instance_property_type(forward, "value"),
        metadata.linked_instance_property_type(reverse, "value")
    );
}

#[cfg(feature = "php-generics-reified")]
#[test]
fn diamond_contract_candidates_keep_individual_pseudo_type_scopes() {
    let output = common::run_php(
        r#"<?php
class Envelope<T> {}
class Root<T> {
    public Envelope<self<T>> $slot;
    public function consume(Envelope<self<T>> $value): string { return "root"; }
}
trait Consumes<T> {
    public Envelope<self<T>> $slot;
    public function consume(Envelope<self<T>> $value): string { return "trait"; }
}
class Combined<T> extends Root<T> { use Consumes<T>; }
$combined = new Combined::<int>();
echo $combined->consume(new Envelope::<Combined<int>>());
echo ":" . $combined->consume(new Envelope::<Root<int>>());
$combined->slot = new Envelope::<Combined<int>>();
$combined->slot = new Envelope::<Root<int>>();
echo $combined->slot instanceof Envelope ? ":property" : ":bad";
"#,
    );
    assert_eq!(output, "trait:trait:property");

    let error = common::run_php_expect_error(
        r#"<?php
class Envelope<T> {}
class Root<T> {
    public function consume(Envelope<self<T>> $value): string { return "root"; }
}
trait Consumes<T> {
    public function consume(Envelope<self<T>> $value): string { return "trait"; }
}
class Unrelated<T> {}
class Combined<T> extends Root<T> { use Consumes<T>; }
$combined = new Combined::<int>();
$combined->consume(new Envelope::<Unrelated<int>>());
"#,
    );
    let rendered = format!("{error:?}");
    assert!(
        rendered.contains("generic") || rendered.contains("reified class type"),
        "{rendered:?}"
    );
}

#[cfg(any(feature = "php-generics-erased", feature = "php-generics-reified"))]
#[test]
fn statically_proven_erasure_equivalent_turbofish_emits_no_runtime_checks() {
    let statements =
        parse("<?php function id<T : int>(T $value): T { return $value; } echo id::<int>(1);")
            .unwrap();
    let result = Compiler::new().compile(&statements).unwrap();
    assert!(result.main.instructions.iter().all(|instruction| {
        !matches!(
            instruction.opcode,
            OpCode::CheckGenericArgs | OpCode::CheckReifiedArgs | OpCode::CheckReifiedReturn
        )
    }));
}

#[cfg(any(feature = "php-generics-erased", feature = "php-generics-reified"))]
#[test]
fn generic_default_check_is_guarded_by_the_default_binding_jump() {
    let statements = parse(
        "<?php function value<T>(T $input = 7): T { return $input; } function plain(int $input = 8): int { return $input; }",
    )
    .unwrap();
    let result = Compiler::new().compile(&statements).unwrap();
    let generic = &result
        .functions
        .iter()
        .find(|(name, _)| name == "value")
        .unwrap()
        .1
        .op_array
        .instructions;
    let bind = generic
        .iter()
        .position(|instruction| instruction.opcode == OpCode::BindDefaultParam)
        .unwrap();
    let assign = generic
        .iter()
        .position(|instruction| instruction.opcode == OpCode::AssignCv)
        .unwrap();
    let check = generic
        .iter()
        .position(|instruction| instruction.opcode == OpCode::CheckGenericDefault)
        .unwrap();
    assert!(bind < assign && assign < check);
    assert_eq!(generic[bind].op2 as usize, check + 1);
    assert_eq!(generic[check].op1, generic[bind].op1);
    assert_eq!(generic[check].extended_value, 0);

    let plain = &result
        .functions
        .iter()
        .find(|(name, _)| name == "plain")
        .unwrap()
        .1
        .op_array
        .instructions;
    assert!(
        plain
            .iter()
            .all(|instruction| instruction.opcode != OpCode::CheckGenericDefault)
    );
}

#[cfg(feature = "php-generics-reified")]
#[test]
fn reified_substitution_that_differs_from_erasure_keeps_boundary_checks() {
    for source in [
        "<?php function id<T>(T $value): T { return $value; } echo id::<int>(1);",
        "<?php class Box<T> {} function id<T>(Box<T> $value): Box<T> { return $value; } id::<int>(new Box::<int>());",
    ] {
        let statements = parse(source).unwrap();
        let result = Compiler::new().compile(&statements).unwrap();
        for opcode in [
            OpCode::CheckGenericArgs,
            OpCode::CheckReifiedArgs,
            OpCode::CheckReifiedReturn,
        ] {
            assert!(
                result
                    .main
                    .instructions
                    .iter()
                    .any(|instruction| instruction.opcode == opcode),
                "missing {opcode:?} for {source:?}"
            );
        }
    }
}

#[cfg(all(feature = "php-generics-erased", not(feature = "php-generics-reified")))]
#[test]
fn erased_runtime_validates_bindings_but_erases_substitution_contracts() {
    let output = common::run_php(
        r#"<?php
function id<T>(T $value): T { return $value; }
function wrong<T>(): T { return "still erased"; }
class Box<T> {
    public function __construct(T $value) { echo $value; }
    public function id(T $value): T { return $value; }
}
echo id::<int>("accepted by mixed erasure");
echo wrong::<int>();
$box = new Box::<int>(" through constructor");
echo $box->id(" through method");
"#,
    );
    assert_eq!(
        output,
        "accepted by mixed erasurestill erased through constructor through method"
    );
}

#[cfg(all(feature = "php-generics-erased", not(feature = "php-generics-reified")))]
#[test]
fn erased_generic_properties_keep_the_erased_storage_contract() {
    let output = common::run_php(
        r#"<?php
class Box<T> { public T $value; }
class ParentBox<T> { public T $value; }
class ChildBox<U> extends ParentBox<U> {}
class IntBox extends ParentBox<int> {}
$box = new Box::<int>();
$box->value = "accepted by mixed erasure";
echo $box->value;
$child = new ChildBox::<int>();
$child->value = " through inherited mixed erasure";
echo $child->value;
$intBox = new IntBox();
$intBox->value = 7;
echo ":" . $intBox->value;
$reflection = new ReflectionObject($box);
echo ":" . count($reflection->getGenericArguments());
echo ":" . count($reflection->getGenericParameters());
"#,
    );
    assert_eq!(
        output,
        "accepted by mixed erasure through inherited mixed erasure:7:0:1"
    );

    let error = common::run_php_expect_error(
        r#"<?php
class IntBox<T : int> { public T $value; }
$box = new IntBox::<int>();
$box->value = "not an int";
"#,
    );
    assert!(format!("{error:?}").contains("bound-erased property IntBox::$value"));

    let inherited_error = common::run_php_expect_error(
        r#"<?php
class IntParent<T : int> { public T $value; }
class IntChild<U : int> extends IntParent<U> {}
$box = new IntChild::<int>();
$box->value = "not an int";
"#,
    );
    assert!(
        format!("{inherited_error:?}").contains("bound-erased property IntChild::$value"),
        "{inherited_error:?}"
    );

    let trait_error = common::run_php_expect_error(
        r#"<?php
trait IntCarries<T : int> { public T $value; }
class IntCarrier<U : int> { use IntCarries<U>; }
$box = new IntCarrier::<int>();
$box->value = "not an int";
"#,
    );
    assert!(
        format!("{trait_error:?}").contains("bound-erased property IntCarrier::$value"),
        "{trait_error:?}"
    );

    for source in [
        "<?php class ParentBox<T> { public T $value; } class IntBox extends ParentBox<int> {} $box = new IntBox(); $box->value = 'bad';",
        "<?php class ParentBox<T> { public T $value; } class ForwardedBox<U : int> extends ParentBox<U> {} $box = new ForwardedBox::<int>(); $box->value = 'bad';",
        "<?php trait Carries<T> { public T $value; } class IntCarrier { use Carries<int>; } $box = new IntCarrier(); $box->value = 'bad';",
        "<?php class ParentBox<T> { public T $value; public function __construct(mixed $value) { $this->value = $value; } } class IntBox extends ParentBox<int> {} new IntBox(1); new IntBox('bad');",
    ] {
        let error = common::run_php_expect_error(source);
        let rendered = format!("{error:?}");
        assert!(rendered.contains("bound-erased property"), "{rendered:?}");
    }
}

#[cfg(all(feature = "php-generics-erased", not(feature = "php-generics-reified")))]
#[test]
fn erased_linker_materializes_forwarded_method_and_constructor_bounds() {
    for source in [
        "<?php class ParentBox<T> { public function id(T $value): T { return $value; } } class ForwardedBox<U : int> extends ParentBox<U> {} $box = new ForwardedBox::<int>(); $box->id('bad');",
        "<?php class ParentBox<T> { public function __construct(T $value) {} } class ForwardedBox<U : int> extends ParentBox<U> {} new ForwardedBox::<int>('bad');",
    ] {
        let error = common::run_php_expect_error(source);
        let rendered = format!("{error:?}");
        assert!(
            rendered.contains("linked generic class type"),
            "{rendered:?}"
        );
    }
}

#[cfg(any(feature = "php-generics-erased", feature = "php-generics-reified"))]
#[test]
fn concrete_descendants_enforce_linked_method_and_constructor_contracts() {
    let output = common::run_php(
        r#"<?php
class ParentBox<T> {
    public function __construct(T $value) { echo $value; }
    public function id(T $value): T { return $value; }
}
class IntBox extends ParentBox<int> {}
class IntGrandchild extends IntBox {}
trait Reads<T> { public function read(T $value): T { return $value; } }
class IntReader { use Reads<int>; }
$box = new IntBox(1);
echo ":" . $box->id(2);
$grandchild = new IntGrandchild(3);
echo ":" . $grandchild->id(4);
echo ":" . (new IntReader())->read(5);
"#,
    );
    assert_eq!(output, "1:23:4:5");

    for (source, expected) in [
        (
            "<?php class ParentBox<T> { public function id(T $value): T { return $value; } } class IntBox extends ParentBox<int> {} $box = new IntBox(); $box->id(1); $box->id('bad');",
            "Argument #1 passed to IntBox::id()",
        ),
        (
            "<?php class ParentBox<T> { public function id(T $value): T { return $value; } } class IntBox extends ParentBox<int> {} class IntGrandchild extends IntBox {} $box = new IntGrandchild(); $box->id('bad');",
            "Argument #1 passed to IntGrandchild::id()",
        ),
        (
            "<?php trait Reads<T> { public function read(T $value): T { return $value; } } class IntReader { use Reads<int>; } $reader = new IntReader(); $reader->read('bad');",
            "Argument #1 passed to IntReader::read()",
        ),
        (
            "<?php class ParentBox<T> { public function __construct(T $value) {} } class IntBox extends ParentBox<int> {} new IntBox('bad');",
            "Argument #1 passed to IntBox::__construct()",
        ),
        (
            "<?php class ParentBox<T> { public function wrong(): T { return 'bad'; } } class IntBox extends ParentBox<int> {} $box = new IntBox(); $box->wrong();",
            "Return value of IntBox::wrong()",
        ),
        (
            "<?php class ParentBox<T> { public function step(T $value): T { return $value + 1; } } class IntBox extends ParentBox<int> {} $box = new IntBox(); $box->step(1); $box->step(9223372036854775807);",
            "Return value of IntBox::step()",
        ),
    ] {
        let error = common::run_php_expect_error(source);
        let rendered = format!("{error:?}");
        assert!(
            rendered.contains(expected),
            "{rendered:?} did not contain {expected:?}"
        );
        assert!(
            rendered.contains("linked generic class type"),
            "{rendered:?}"
        );
    }
}

#[cfg(any(feature = "php-generics-erased", feature = "php-generics-reified"))]
#[test]
fn concrete_descendants_erase_method_parameters_after_linking_class_bounds() {
    let output = common::run_php(
        r#"<?php
class ParentBox<T> {
    public function id<U : T>(U $value): U { return $value; }
}
class IntBox extends ParentBox<int> {}
echo (new IntBox())->id::<int>(7);
"#,
    );
    assert_eq!(output, "7");

    let error = common::run_php_expect_error(
        "<?php class ParentBox<T> { public function id<U : T>(U $value): U { return $value; } } class IntBox extends ParentBox<int> {} $box = new IntBox(); $box->id('bad');",
    );
    let rendered = format!("{error:?}");
    assert!(
        rendered.contains("Argument #1 passed to IntBox::id()"),
        "{rendered:?}"
    );
    assert!(
        rendered.contains("linked generic class type"),
        "{rendered:?}"
    );
}

#[cfg(feature = "php-generics-reified")]
#[test]
fn reified_runtime_enforces_substituted_argument_and_return_contracts() {
    let argument_error = common::run_php_expect_error(
        r#"<?php
function id<T>(T $value): T { return $value; }
id::<int>("not an int");
"#,
    );
    assert!(format!("{argument_error:?}").contains("does not match its reified generic type"));

    let return_error = common::run_php_expect_error(
        r#"<?php
function wrong<T>(): T { return "not an int"; }
wrong::<int>();
"#,
    );
    assert!(format!("{return_error:?}").contains("Return value of wrong"));

    let output = common::run_php(
        r#"<?php
function inner<U>(U $value): U { return $value; }
function outer<T>(T $value): T { return $value; }
echo outer::<int>(inner::<int>(9));
"#,
    );
    assert_eq!(output, "9");
}

#[cfg(feature = "php-generics-reified")]
#[test]
fn reified_runtime_checks_omitted_defaults_after_materialization() {
    let output = common::run_php(
        r#"<?php
function valid<T>(T $value = 7): T { return $value; }
function named<T>(T $value = 10, string $tail = "z"): string { return $value . $tail; }
class MethodDefaults {
    public function valid<T>(T $value = 8): T { return $value; }
}
class InstanceDefaults<T> {
    public function valid(T $value = 9): T { return $value; }
}
echo valid::<int>() . ":";
echo named::<int>(tail: "n") . ":";
$methods = new MethodDefaults();
echo $methods->valid::<int>() . ":";
$instance = new InstanceDefaults::<int>();
echo $instance->valid();
"#,
    );
    assert_eq!(output, "7:10n:8:9");

    let generator_output = common::run_php(
        r#"<?php
function values<T>(T $first = 11, T $second = 12) {
    yield $first;
    yield $second;
}
function delegatedValue($value) { yield $value; }
function delegatedValues<T>(T $value = 14) { yield from delegatedValue($value); }
class GeneratorDefaults<T> {
    public function values(T $value = 13) { yield $value; }
}
foreach (values::<int>() as $value) { echo $value . ":"; }
$defaults = new GeneratorDefaults::<int>();
foreach ($defaults->values() as $value) { echo $value . ":"; }
foreach (delegatedValues::<int>() as $value) { echo $value; }
"#,
    );
    assert_eq!(generator_output, "11:12:13:14");

    for source in [
        r#"<?php function consume<T>(T $value = "bad"): string { return "body"; } consume::<int>();"#,
        r#"<?php class Defaults { public function consume<T>(T $value = "bad"): string { return "body"; } } $value = new Defaults(); $value->consume::<int>();"#,
        r#"<?php class Defaults { public static function consume<T>(T $value = "bad"): string { return "body"; } } Defaults::consume::<int>();"#,
        r#"<?php $consume = function<T>(T $value = "bad"): string { return "body"; }; $consume::<int>();"#,
        r#"<?php function nested<U>(U $value): U { return $value; } function consume<T>(T $value = nested::<string>("bad")): string { return "body"; } consume::<int>();"#,
        r#"<?php class Defaults<T> { public function consume(T $value = "bad"): string { return "body"; } } $value = new Defaults::<int>(); $value->consume();"#,
        r#"<?php class ParentDefaults<T> { public function consume(T $value = "bad"): string { return "body"; } } class ChildDefaults<U> extends ParentDefaults<U> {} $value = new ChildDefaults::<int>(); $value->consume();"#,
        r#"<?php trait TraitDefaults<T> { public function consume(T $value = "bad"): string { return "body"; } } class Defaults<T> { use TraitDefaults<T>; } $value = new Defaults::<int>(); $value->consume();"#,
        r#"<?php class Defaults<T> { public function __construct(T $value = "bad") {} } new Defaults::<int>();"#,
        r#"<?php class NestedBox<T> {} function consume<T>(NestedBox<T> $value = new NestedBox::<string>()): string { return "body"; } consume::<int>();"#,
        r#"<?php function values<T>(T $value = "bad") { yield $value; } foreach (values::<int>() as $value) {}"#,
        r#"<?php class Defaults { public function values<T>(T $value = "bad") { yield $value; } } $defaults = new Defaults(); foreach ($defaults->values::<int>() as $value) {}"#,
        r#"<?php class Defaults<T> { public function values(T $value = "bad") { yield $value; } } $defaults = new Defaults::<int>(); foreach ($defaults->values() as $value) {}"#,
    ] {
        let error = common::run_php_expect_error(source);
        let rendered = format!("{error:?}");
        let normalized = rendered.to_ascii_lowercase();
        assert!(
            normalized.contains("default")
                && (normalized.contains("generic") || normalized.contains("class type")),
            "{rendered:?}"
        );
    }
}

#[cfg(any(feature = "php-generics-erased", feature = "php-generics-reified"))]
#[test]
fn linked_instance_contracts_check_omitted_defaults() {
    let output = common::run_php(
        r#"<?php
class ParentDefaults<T> {
    public function value(T $value = 7): T { return $value; }
}
class IntDefaults extends ParentDefaults<int> {}
$value = new IntDefaults();
echo $value->value();
"#,
    );
    assert_eq!(output, "7");

    let generator_output = common::run_php(
        r#"<?php
class ParentGeneratorDefaults<T> {
    public function values(T $value = 8) { yield $value; }
}
class IntGeneratorDefaults extends ParentGeneratorDefaults<int> {}
$value = new IntGeneratorDefaults();
foreach ($value->values() as $item) { echo $item; }
"#,
    );
    assert_eq!(generator_output, "8");

    let error = common::run_php_expect_error(
        r#"<?php
class ParentDefaults<T> {
    public function consume(T $value = "bad"): string { return "body"; }
}
class IntDefaults extends ParentDefaults<int> {}
$value = new IntDefaults();
$value->consume();
"#,
    );
    let rendered = format!("{error:?}");
    let normalized = rendered.to_ascii_lowercase();
    assert!(
        normalized.contains("default")
            && (normalized.contains("generic") || normalized.contains("class type")),
        "{rendered:?}"
    );

    let generator_error = common::run_php_expect_error(
        r#"<?php
class ParentGeneratorDefaults<T> {
    public function values(T $value = "bad") { yield $value; }
}
class IntGeneratorDefaults extends ParentGeneratorDefaults<int> {}
$value = new IntGeneratorDefaults();
foreach ($value->values() as $item) {}
"#,
    );
    let rendered = format!("{generator_error:?}");
    let normalized = rendered.to_ascii_lowercase();
    assert!(
        normalized.contains("default")
            && (normalized.contains("generic") || normalized.contains("class type")),
        "{rendered:?}"
    );

    let constructor_error = common::run_php_expect_error(
        r#"<?php
class ParentDefaults<T> { public function __construct(T $value = "bad") {} }
class IntDefaults extends ParentDefaults<int> {}
new IntDefaults();
"#,
    );
    let rendered = format!("{constructor_error:?}");
    assert!(
        rendered.to_ascii_lowercase().contains("default"),
        "{rendered:?}"
    );
}

#[cfg(feature = "php-generics-reified")]
#[test]
fn reified_runtime_enforces_nested_named_type_arguments() {
    let output = common::run_php(
        r#"<?php
class NestedBox<T> {}
class NestedChild<U> extends NestedBox<U> {}
class NestedInt extends NestedBox<int> {}
function accept<T>(T $value): T { return $value; }
function acceptNested<T>(NestedBox<T> $value): NestedBox<T> { return $value; }
class Holder<T> {
    public T $value;
    public function take(T $value): T { return $value; }
}
class ExplicitHost {
    public function take<T>(NestedBox<T> $value): NestedBox<T> { return $value; }
}

$int = new NestedBox::<int>();
$child = new NestedChild::<int>();
$concrete = new NestedInt();
$holder = new Holder::<NestedBox<int>>();
$holder->value = $concrete;
echo accept::<NestedBox<int>>($int) instanceof NestedBox ? "direct:" : "bad:";
echo accept::<NestedBox<int>>($child) instanceof NestedBox ? "ancestor:" : "bad:";
echo accept::<NestedBox<int>>($concrete) instanceof NestedBox ? "concrete:" : "bad:";
echo acceptNested::<int>($int) instanceof NestedBox ? "parameter:" : "bad:";
echo $holder->take($concrete) instanceof NestedBox ? "method:" : "bad:";
echo (new ExplicitHost())->take::<int>($int) instanceof NestedBox ? "explicit:" : "bad:";
echo $holder->value instanceof NestedBox ? "property" : "bad";
"#,
    );
    assert_eq!(
        output,
        "direct:ancestor:concrete:parameter:method:explicit:property"
    );

    for (source, expected) in [
        (
            "<?php class NestedBox<T> {} function accept<T>(T $value): T { return $value; } accept::<NestedBox<int>>(new NestedBox::<string>());",
            "does not match its reified generic type",
        ),
        (
            "<?php class NestedBox<T> {} function accept<T>(T $value): T { return $value; } accept::<NestedBox<int>>(new NestedBox());",
            "does not match its reified generic type",
        ),
        (
            "<?php class NestedBox<T> {} function accept<T>(NestedBox<T> $value): NestedBox<T> { return $value; } accept::<int>(new NestedBox::<string>());",
            "does not match its reified generic type",
        ),
        (
            "<?php class NestedBox<T> {} class NestedString extends NestedBox<string> {} function accept<T>(T $value): T { return $value; } accept::<NestedBox<int>>(new NestedString());",
            "does not match its reified generic type",
        ),
        (
            "<?php class NestedBox<T> {} function wrong<T>(): T { return new NestedBox::<string>(); } wrong::<NestedBox<int>>();",
            "Return value of wrong",
        ),
        (
            "<?php class NestedBox<T> {} class Holder<T> { public T $value; } $holder = new Holder::<NestedBox<int>>(); $holder->value = new NestedBox::<string>();",
            "reified property Holder::$value",
        ),
        (
            "<?php class NestedBox<T> {} class Holder<T> { public function take(T $value): T { return $value; } } $holder = new Holder::<NestedBox<int>>(); $holder->take(new NestedBox::<string>());",
            "Argument #1 passed to Holder::take()",
        ),
        (
            "<?php class NestedBox<T> {} class NestedHost { public function take<T>(NestedBox<T> $value): NestedBox<T> { return $value; } } $host = new NestedHost(); $host->take::<int>(new NestedBox::<string>());",
            "does not match its reified generic type",
        ),
    ] {
        let error = common::run_php_expect_error(source);
        let rendered = format!("{error:?}");
        assert!(
            rendered.contains(expected),
            "{rendered:?} did not contain {expected:?}"
        );
    }
}

#[cfg(feature = "php-generics-reified")]
#[test]
fn reified_runtime_checks_every_positional_and_named_variadic_argument() {
    let output = common::run_php(
        r#"<?php
function first<T>(T ...$values): T { return $values[0]; }
class VariadicHost {
    public function first<T>(T ...$values): T { return $values[0]; }
}
$closure = function<T>(T ...$values): T { return $values[0]; };
echo first::<int>(1, 2, 3);
echo (new VariadicHost())->first::<string>("m", "n");
echo $closure::<int>(4, 5);
echo first("ordinary", 6);
"#,
    );
    assert_eq!(output, "1m4ordinary");

    for (source, expected) in [
        (
            "<?php function first<T>(T ...$values): T { return $values[0]; } first::<int>(1, 2, 'bad');",
            "Variadic argument #3 passed to first",
        ),
        (
            "<?php class Host { public function first<T>(T ...$values): T { return $values[0]; } } $host = new Host(); $host->first::<int>(1, 'bad');",
            "Variadic argument #2 passed to Host::first",
        ),
        (
            "<?php $first = function<T>(T ...$values): T { return $values[0]; }; $first::<int>(1, 'bad');",
            "Variadic argument #2 passed to __closure_",
        ),
        (
            "<?php function first<T>(T ...$values): T { return $values[0]; } first::<int>(valid: 1, invalid: 'bad');",
            "Named variadic argument $invalid passed to first",
        ),
    ] {
        let error = common::run_php_expect_error(source);
        let rendered = format!("{error:?}");
        assert!(rendered.contains(expected), "{rendered:?}");
    }
}

#[cfg(all(feature = "php-generics-erased", not(feature = "php-generics-reified")))]
#[test]
fn erased_runtime_keeps_unbounded_variadic_arguments_erased_to_mixed() {
    let output = common::run_php(
        r#"<?php
function first<T>(T ...$values): T { return $values[0]; }
$closure = function<T>(T ...$values): T { return $values[0]; };
echo first::<int>("erased", 2);
echo $closure::<int>(" closure", 3);
first::<int>(named: "accepted");
echo " named";
"#,
    );
    assert_eq!(output, "erased closure named");
}

#[cfg(feature = "php-generics-reified")]
#[test]
fn reified_instances_enforce_property_bindings_and_clone_identity() {
    let output = common::run_php(
        r#"<?php
class Box<T> { public T $value; }
class Pair<T, U = T> {}
class Nested<T> {}
$ints = new Box::<int>();
$strings = new Box::<string>();
$ordinary = new Box();
$defaulted = new Pair::<int>();
$nested = new Nested::<Pair<int, string>>();
$ints->value = 1;
$strings->value = "s";
$clone = clone $ints;
$clone->value = 2;
echo $ints->value . $strings->value . $clone->value;
$intArguments = (new ReflectionObject($ints))->getGenericArguments();
$stringArguments = (new ReflectionObject($strings))->getGenericArguments();
echo ":" . get_class($intArguments[0]) . ":" . $intArguments[0]->getName();
echo ":" . $stringArguments[0]->getName();
$cloneArguments = (new ReflectionObject($clone))->getGenericArguments();
echo ":" . $cloneArguments[0]->getName();
echo ":" . count((new ReflectionObject($ordinary))->getGenericArguments());
$defaultArguments = (new ReflectionObject($defaulted))->getGenericArguments();
echo ":" . $defaultArguments[0]->getName() . ":" . $defaultArguments[1]->getName();
$nestedArgument = (new ReflectionObject($nested))->getGenericArguments()[0];
echo ":" . $nestedArgument->getName() . ":";
echo $nestedArgument->getGenericArguments()[0]->getName() . ":";
echo $nestedArgument->getGenericArguments()[1]->getName();
function assignBox($box, $value) { $box->value = $value; }
assignBox($ints, 3);
assignBox($strings, "t");
echo ":" . $ints->value . ":" . $strings->value;
"#,
    );
    assert_eq!(
        output,
        "1s2:ReflectionNamedType:int:string:int:0:int:int:Pair:int:string:3:t"
    );

    let promoted_error = common::run_php_expect_error(
        r#"<?php
class Promoted<T> {
    public function __construct(public T $value) {}
}
$valid = new Promoted::<int>(1);
$invalid = new Promoted::<int>("not an int");
"#,
    );
    assert!(
        format!("{promoted_error:?}").contains("Argument #1 passed to Promoted::__construct()"),
        "{promoted_error:?}"
    );

    let visibility_error = common::run_php_expect_error(
        r#"<?php
class PrivateBox<T> { private T $value; }
$box = new PrivateBox::<int>();
$box->value = "not an int";
"#,
    );
    assert!(format!("{visibility_error:?}").contains("Cannot access private property"));

    for source in [
        r#"<?php
class Box<T> { public T $value; }
$box = new Box::<int>();
$box->value = "not an int";
"#,
        r#"<?php
class Box<T> { public T $value; }
$box = new Box::<int>();
$clone = clone $box;
$clone->value = "not an int";
"#,
        r#"<?php
class Box<T> { public T $value = "not an int"; }
$box = new Box::<int>();
"#,
        r#"<?php
class Box<T> { public T $value; }
function assignBox($box, $value) { $box->value = $value; }
$strings = new Box::<string>();
$ints = new Box::<int>();
assignBox($strings, "valid and caches the property site");
assignBox($ints, "not an int");
"#,
    ] {
        let error = common::run_php_expect_error(source);
        assert!(
            format!("{error:?}").contains("reified property Box::$value"),
            "{error:?}"
        );
    }
}

#[cfg(feature = "php-generics-reified")]
#[test]
fn reified_instances_substitute_inherited_property_bindings() {
    let output = common::run_php(
        r#"<?php
class ParentBox<T> {
    public T $value;
    public function __construct(mixed $value) { $this->value = $value; }
}
class ChildBox<U> extends ParentBox<U> {}
class GrandchildBox<V> extends ChildBox<V> {}
class IntBox extends ParentBox<int> {}
$child = new ChildBox::<int>(1);
$child->value = 2;
$grandchild = new GrandchildBox::<string>("three");
$grandchild->value = "four";
$intBox = new IntBox(5);
$intBox->value = 6;
echo $child->value . ":" . $grandchild->value . ":" . $intBox->value;
"#,
    );
    assert_eq!(output, "2:four:6");

    for source in [
        "<?php class ParentBox<T> { public T $value; } class ChildBox<U> extends ParentBox<U> {} $box = new ChildBox::<int>(); $box->value = 'bad';",
        "<?php class ParentBox<T> { public T $value; public function __construct(mixed $value) { $this->value = $value; } } class ChildBox<U> extends ParentBox<U> {} new ChildBox::<int>(1); new ChildBox::<int>('bad');",
        "<?php class ParentBox<T> { public T $value; } class ChildBox<U> extends ParentBox<U> {} class GrandchildBox<V> extends ChildBox<V> {} $box = new GrandchildBox::<int>(); $box->value = 'bad';",
        "<?php trait Carries<T> { public T $value; } class Carrier<U> { use Carries<U>; } $box = new Carrier::<int>(); $box->value = 'bad';",
        "<?php class ParentBox<T> { public T $value; } class ChildBox<U> extends ParentBox<U> {} function assign($box, $value) { $box->value = $value; } $strings = new ChildBox::<string>(); $ints = new ChildBox::<int>(); assign($strings, 'valid'); assign($ints, 'bad');",
    ] {
        let error = common::run_php_expect_error(source);
        let rendered = format!("{error:?}");
        assert!(
            rendered.contains("reified property") && rendered.contains("::$value"),
            "{rendered:?}"
        );
    }

    for source in [
        "<?php class ParentBox<T> { public T $value; } class IntBox extends ParentBox<int> {} $box = new IntBox(); $box->value = 'bad';",
        "<?php class ParentBox<T> { public T $value; } class IntBox extends ParentBox<int> {} class ConcreteGrandchild extends IntBox {} $box = new ConcreteGrandchild(); $box->value = 'bad';",
        "<?php trait Carries<T> { public T $value; } class IntCarrier { use Carries<int>; } $box = new IntCarrier(); $box->value = 'bad';",
        "<?php class ParentBox<T> { public T $value; public function __construct(mixed $value) { $this->value = $value; } } class IntBox extends ParentBox<int> {} new IntBox(1); new IntBox('bad');",
    ] {
        let error = common::run_php_expect_error(source);
        let rendered = format!("{error:?}");
        assert!(rendered.contains("bound-erased property"), "{rendered:?}");
    }
}

#[cfg(feature = "php-generics-reified")]
#[test]
fn reified_instances_enforce_substituted_method_contracts() {
    let output = common::run_php(
        r#"<?php
class ParentBox<T> {
    public function id(T $value): T { return $value; }
    public function first(T ...$values): T { return $values[0]; }
}
class ChildBox<U> extends ParentBox<U> {}
class WideBox<V> extends ParentBox<V> {
    public function id(mixed $value): V { return 10; }
}
class ThrowBox<W> {
    public function id(W $value): W {
        if ($value === 0) { throw new Exception("expected"); }
        return $value;
    }
}
class StepBox<X> {
    public function step(X $value): X { return $value + 1; }
}
$box = new ChildBox::<int>();
echo $box->id($box->id(8));
echo $box->first(9, 10);
$wide = new WideBox::<int>();
echo $wide->id("accepted by contravariance");
$throw = new ThrowBox::<int>();
try { $throw->id(0); } catch (Exception $error) {}
echo $throw->id(11);
$step = new StepBox::<int>();
echo $step->step(12);
"#,
    );
    assert_eq!(output, "89101113");

    for (source, expected) in [
        (
            "<?php class Box<T> { public function id(T $value): T { return $value; } } $box = new Box::<int>(); $box->id('bad');",
            "Argument #1 passed to Box::id()",
        ),
        (
            "<?php class ParentBox<T> { public function id(T $value): T { return $value; } } class ChildBox<U> extends ParentBox<U> {} $box = new ChildBox::<int>(); $box->id('bad');",
            "Argument #1 passed to ParentBox::id()",
        ),
        (
            "<?php class Box<T> { public function wrong(): T { return 'bad'; } } $box = new Box::<int>(); $box->wrong();",
            "Return value of Box::wrong()",
        ),
        (
            "<?php class Box<T> { public function first(T ...$values): T { return $values[0]; } } $box = new Box::<int>(); $box->first(1, 'bad');",
            "Variadic argument #2 passed to Box::first()",
        ),
        (
            "<?php class StepBox<T> { public function step(T $value): T { return $value + 1; } } $box = new StepBox::<int>(); $box->step(9223372036854775807);",
            "Return value of StepBox::step()",
        ),
    ] {
        let error = common::run_php_expect_error(source);
        let rendered = format!("{error:?}");
        assert!(
            rendered.contains(expected),
            "{rendered:?} did not contain {expected:?}"
        );
        assert!(rendered.contains("reified class type"), "{rendered:?}");
    }
}

#[cfg(feature = "php-generics-reified")]
#[test]
fn reified_instances_erase_method_parameters_to_substituted_class_bounds() {
    let error = common::run_php_expect_error(
        "<?php class Box<T> { public function id<U : T>(U $value): U { return $value; } } $box = new Box::<int>(); $box->id('bad');",
    );
    let rendered = format!("{error:?}");
    assert!(
        rendered.contains("Argument #1 passed to Box::id()"),
        "{rendered:?}"
    );
    assert!(rendered.contains("reified class type"), "{rendered:?}");
}

#[cfg(feature = "php-generics-reified")]
#[test]
fn hot_method_dispatch_rechecks_a_new_receivers_reified_contract() {
    let error = common::run_php_expect_error(
        r#"<?php
class StepBox<T> {
    public function step(T $value): T { return $value + 1; }
}
function runSteps($box) {
    $value = 0;
    for ($i = 0; $i < 100; $i++) {
        $value = $box->step($value);
    }
    return $value;
}
runSteps(new StepBox());
runSteps(new StepBox::<string>());
"#,
    );
    let rendered = format!("{error:?}");
    assert!(
        rendered.contains("Argument #1 passed to StepBox::step()"),
        "{rendered:?}"
    );
    assert!(rendered.contains("reified class type"), "{rendered:?}");
}

#[cfg(feature = "php-generics-reified")]
#[test]
fn reified_instances_enforce_substituted_constructor_contracts() {
    let output = common::run_php(
        r#"<?php
class ParentBox<T> {
    public function __construct(T $value) { echo $value; }
}
class ChildBox<U> extends ParentBox<U> {}
class ThrowBox<V> {
    public function __construct(V $value) {
        if ($value === 0) { throw new Exception("expected"); }
        echo $value;
    }
}
new ParentBox::<int>(12);
new ChildBox::<string>(" inherited");
try { new ThrowBox::<int>(0); } catch (Exception $error) {}
new ThrowBox(" erased after throw");
"#,
    );
    assert_eq!(output, "12 inherited erased after throw");

    for (source, expected) in [
        (
            "<?php class Box<T> { public function __construct(T $value) {} } new Box::<int>('bad');",
            "Argument #1 passed to Box::__construct()",
        ),
        (
            "<?php class ParentBox<T> { public function __construct(T $value) {} } class ChildBox<U> extends ParentBox<U> {} new ChildBox::<int>('bad');",
            "Argument #1 passed to ParentBox::__construct()",
        ),
        (
            "<?php class Box<T> { public T $value; public function __construct(T $value) { $this->value = $value; } } new Box::<int>(1); new Box::<int>('bad');",
            "Argument #1 passed to Box::__construct()",
        ),
        (
            "<?php class Box<T> { public T $value; public function __construct(mixed $value) { $this->value = $value; } } new Box::<int>(1); new Box::<int>('bad');",
            "reified property Box::$value",
        ),
    ] {
        let error = common::run_php_expect_error(source);
        let rendered = format!("{error:?}");
        assert!(
            rendered.contains(expected),
            "{rendered:?} did not contain {expected:?}"
        );
        assert!(
            rendered.contains("reified class type") || rendered.contains("reified property"),
            "{rendered:?}"
        );
    }
}

#[cfg(any(feature = "php-generics-erased", feature = "php-generics-reified"))]
#[test]
fn compiler_preserves_interned_pre_erasure_metadata_off_hot_structures() {
    let statements = parse(
        r#"<?php
interface Source<+T> {}
trait Holder<T : object> {}
class Box<T : object = stdClass> {
    public T $value;
    public function pair<-L, +R : Box<stdClass>>(L $left): R { return $left; }
    public function copy(): static<T> { return $this; }
}
function id<T : Box<Box<int>>>(T $value): T { return $value; }
"#,
    )
    .unwrap();
    let result = Compiler::new().compile(&statements).unwrap();
    let metadata = &result.generic_metadata;

    let source = metadata
        .find(GenericDeclarationKind::Interface, "source")
        .unwrap();
    assert_eq!(source.parameters.len(), 1);
    assert_eq!(source.parameters[0].variance, GenericVariance::Covariant);

    let pair = metadata
        .find(GenericDeclarationKind::Method, "Box::pair")
        .unwrap();
    assert_eq!(pair.parameters.len(), 2);
    assert_eq!(pair.parameters[0].variance, GenericVariance::Contravariant);
    assert_eq!(pair.parameters[1].variance, GenericVariance::Covariant);
    assert!(matches!(
        pair.parameters[1].bound,
        Some(GenericType::Named { .. })
    ));

    let id = metadata
        .find(GenericDeclarationKind::Function, "id")
        .unwrap();
    let Some(GenericType::Named { arguments, .. }) = &id.parameters[0].bound else {
        panic!("expected nested named generic bound");
    };
    assert_eq!(arguments.len(), 1);
    assert!(matches!(arguments[0], GenericType::Named { .. }));

    let boxed = metadata.find(GenericDeclarationKind::Class, "Box").unwrap();
    assert_eq!(boxed.properties.len(), 1);
    assert!(matches!(
        boxed.properties[0].value_type,
        GenericType::Parameter(0)
    ));
    assert_eq!(boxed.methods.len(), 2);
    assert_eq!(boxed.methods[0].value_parameters.len(), 1);
    let copy = boxed
        .methods
        .iter()
        .find(|method| metadata.symbol(method.name) == Some("copy"))
        .unwrap();
    let Some(GenericType::Named { name, arguments }) = copy.return_type.as_ref() else {
        panic!("expected static<T> method metadata");
    };
    assert_eq!(metadata.symbol(*name), Some("static"));
    assert!(matches!(arguments.as_ref(), [GenericType::Parameter(0)]));

    let inheritance_statements = parse(
        "<?php interface Source<T> {} trait Holder<T> {} class Child<U> implements Source<U> { use Holder<U>; }",
    )
    .unwrap();
    let inheritance_result = Compiler::new().compile(&inheritance_statements).unwrap();
    assert_eq!(inheritance_result.generic_metadata.inheritances().len(), 2);
    assert!(
        inheritance_result
            .generic_metadata
            .inheritances()
            .iter()
            .all(|inheritance| matches!(inheritance.arguments[0], GenericType::Parameter(0)))
    );
}

#[cfg(any(feature = "php-generics-erased", feature = "php-generics-reified"))]
#[test]
fn shared_reflection_exposes_interned_declarations_and_runtime_capabilities() {
    let output = common::run_php(
        r#"<?php
function id<T : int>(T $value): T { return $value; }
class Box<T : object = stdClass> {
    public function map<+U : string>(): U { return "value"; }
}
$function = new ReflectionFunction("id");
$functionParameters = $function->getGenericParameters();
echo $function->isGeneric() ? "yes:" : "no:";
echo $functionParameters[0]->getName() . ":" . $functionParameters[0]->getBound()->getName() . ":";
$class = new ReflectionClass("Box");
$classParameters = $class->getGenericParameters();
echo $classParameters[0]->getDefault()->getName() . ":";
$method = new ReflectionMethod("Box", "map");
$methodParameters = $method->getGenericParameters();
echo $methodParameters[0]->getVariance()->name . ":";
echo count($method->getGenericRuntimeModes());
"#,
    );
    let mode_count = usize::from(cfg!(feature = "php-generics-erased"))
        + usize::from(cfg!(feature = "php-generics-reified"));
    assert_eq!(output, format!("yes:T:int:stdClass:Covariant:{mode_count}"));
}

#[cfg(any(feature = "php-generics-erased", feature = "php-generics-reified"))]
#[test]
fn reflection_generic_parameter_objects_follow_the_rfc_surface() {
    let output = common::run_php(
        r#"<?php
interface ReflectedInterface<+T : object = stdClass> {}
trait ReflectedTrait<-U> {}
class ReflectedClass<+A : object = stdClass, +B : object = stdClass> {}
function reflectedFunction<F>() {}
class ReflectedMethodHost<C : object> {
    public function run<V : C, W : V = V>() {}
}

$class = new ReflectionClass("ReflectedClass");
$parameters = $class->getGenericParameters();
$parameter = $parameters[0];
echo get_class($parameter) . ":" . $parameter->name . ":";
echo $parameter->getName() . ":" . $parameter->getPosition() . ":";
echo get_class($parameter->getVariance()) . ":" . $parameter->getVariance()->name . ":";
echo $parameter->hasBound() ? $parameter->getBound()->getName() . ":" : "missing:";
echo $parameter->hasDefault() ? $parameter->getDefault()->getName() . ":" : "missing:";
echo get_class($parameter->getDeclaringEntity()) . ":" . $parameter->__toString() . ":";
echo ($parameters[0]->getVariance() === $parameters[1]->getVariance()) ? "singleton:" : "split:";
echo ($parameter->getVariance() === ReflectionGenericVariance::Covariant) ? "case:" : "missing:";

$interface = new ReflectionClass("ReflectedInterface");
$trait = new ReflectionClass("ReflectedTrait");
echo $interface->isGeneric() ? "interface:" : "missing:";
echo $trait->isGeneric() ? $trait->getGenericParameters()[0]->getVariance()->name . ":" : "missing:";

$function = new ReflectionFunction("reflectedFunction");
$functionParameter = $function->getGenericParameters()[0];
echo get_class($functionParameter->getDeclaringEntity()) . ":";
$method = new ReflectionMethod("ReflectedMethodHost", "run");
$methodParameters = $method->getGenericParameters();
$methodParameter = $methodParameters[0];
echo get_class($methodParameter->getDeclaringEntity()) . ":";
$classBound = $methodParameter->getBound();
echo $classBound->name . ":" . get_class($classBound->getTypeParameter()->getDeclaringEntity()) . ":";
$methodBound = $methodParameters[1]->getBound();
echo $methodBound->name . ":" . get_class($methodBound->getTypeParameter()->getDeclaringEntity()) . ":";
echo $methodParameters[1]->getDefault()->getName();

try {
    $functionParameter->getDefault();
} catch (ReflectionException $error) {
    echo ":caught";
}
"#,
    );
    assert_eq!(
        output,
        "ReflectionGenericTypeParameter:A:A:0:ReflectionGenericVariance:Covariant:object:stdClass:ReflectionClass:A:singleton:case:interface:Contravariant:ReflectionFunction:ReflectionMethod:C:ReflectionClass:V:ReflectionMethod:V:caught"
    );
}

#[cfg(any(feature = "php-generics-erased", feature = "php-generics-reified"))]
#[test]
fn reflection_function_accepts_generic_closure_and_arrow_values() {
    let output = common::run_php(
        r#"<?php
$closure = function<C : object = stdClass>() {};
$arrow = fn<A : string = string>() => "value";
$plain = function() {};

$closureReflection = new ReflectionFunction($closure);
$closureParameter = $closureReflection->getGenericParameters()[0];
echo ($closureReflection instanceof ReflectionFunctionAbstract) ? "function:" : "missing:";
echo $closureReflection->isGeneric() ? "generic:" : "plain:";
echo $closureParameter->getName() . ":";
echo $closureParameter->getBound()->getName() . ":";
echo $closureParameter->getDefault()->getName() . ":";
$declaringClosure = $closureParameter->getDeclaringEntity();
echo $declaringClosure->isGeneric() ? $declaringClosure->getGenericParameters()[0]->getName() . ":" : "missing:";

$arrowReflection = new ReflectionFunction($arrow);
$arrowParameter = $arrowReflection->getGenericParameters()[0];
echo $arrowReflection->isGeneric() ? "arrow:" : "missing:";
echo $arrowParameter->getName() . ":";
echo $arrowParameter->getBound()->getName() . ":";
echo $arrowParameter->getDefault()->getName() . ":";
echo get_class($arrowParameter->getDeclaringEntity()) . ":";

$plainReflection = new ReflectionFunction($plain);
echo $plainReflection->isGeneric() ? "missing:" : "plain:";
echo count($plainReflection->getGenericParameters());
"#,
    );
    assert_eq!(
        output,
        "function:generic:C:object:stdClass:C:arrow:A:string:string:ReflectionFunction:plain:0"
    );
}

#[cfg(any(feature = "php-generics-erased", feature = "php-generics-reified"))]
#[test]
fn generic_reflection_follows_the_builtin_class_hierarchy() {
    let output = common::run_php(
        r#"<?php
class ReflectedHierarchy<T> {
    public function map<U : T>() {}
}
$class = new ReflectionClass("ReflectedHierarchy");
$method = new ReflectionMethod("ReflectedHierarchy", "map");
$classParameter = $class->getGenericParameters()[0];
$methodBound = $method->getGenericParameters()[0]->getBound();
echo ($class instanceof Reflector) ? "class:" : "missing:";
echo ($method instanceof ReflectionFunctionAbstract) ? "method:" : "missing:";
echo ($method instanceof Reflector) ? "reflector:" : "missing:";
echo ($classParameter instanceof Reflector) ? "parameter:" : "missing:";
echo ($methodBound instanceof ReflectionType) ? "type:" : "missing:";
echo ($methodBound instanceof ReflectionTypeParameterReference) ? "reference" : "missing";
"#,
    );
    assert_eq!(output, "class:method:reflector:parameter:type:reference");
}

#[cfg(any(feature = "php-generics-erased", feature = "php-generics-reified"))]
#[test]
fn generic_ancestor_reflection_distinguishes_empty_bindings_and_invalid_targets() {
    let output = common::run_php(
        r#"<?php
class PlainParent {}
class PlainChild extends PlainParent {}
interface PlainInterface {}
class PlainImplementation implements PlainInterface {}
trait PlainTrait {}
class PlainTraitUser { use PlainTrait; }

echo count((new ReflectionClass("PlainChild"))->getGenericArgumentsForParentClass()) . ":";
echo count((new ReflectionClass("PlainImplementation"))->getGenericArgumentsForParentInterface("PlainInterface")) . ":";
echo count((new ReflectionClass("PlainTraitUser"))->getGenericArgumentsForUsedTrait("PlainTrait")) . ":";

foreach (["parent", "interface", "trait"] as $case) {
    try {
        if ($case === "parent") {
            $reflection = new ReflectionClass("PlainParent");
            $reflection->getGenericArgumentsForParentClass();
        } elseif ($case === "interface") {
            $reflection = new ReflectionClass("PlainChild");
            $reflection->getGenericArgumentsForParentInterface("PlainInterface");
        } else {
            $reflection = new ReflectionClass("PlainChild");
            $reflection->getGenericArgumentsForUsedTrait("PlainTrait");
        }
    } catch (ReflectionException $error) {
        echo "caught:";
    }
}
"#,
    );
    assert_eq!(output, "0:0:0:caught:caught:caught:");
}

#[cfg(any(feature = "php-generics-erased", feature = "php-generics-reified"))]
#[test]
fn parser_enforces_declaration_invariants() {
    let cases = [
        (
            "<?php function bad<T, T>() {}",
            "Duplicate generic parameter T",
        ),
        (
            "<?php function bad<T = int, U>() {}",
            "Required generic parameter U follows an optional parameter",
        ),
        (
            "<?php function bad<T : T>() {}",
            "cannot use itself as a top-level bound",
        ),
        (
            "<?php function bad<T : int = string>() {}",
            "does not satisfy its bound",
        ),
        (
            "<?php function bad<T = U, U = int>() {}",
            "references U before it is declared",
        ),
        (
            "<?php class C<T> { public function bad<T>() {} }",
            "shadows an outer generic parameter",
        ),
        (
            "<?php class C { public static $value; } C::value::<int>;",
            "must be followed by a method call",
        ),
        (
            "<?php class C<T> { public function bad(static<T> $value) {} }",
            "static is only allowed as a return type",
        ),
        (
            "<?php class C<T> { public function bad(C<T>|static<T> $value) {} }",
            "static is only allowed as a return type",
        ),
    ];
    for (source, expected) in cases {
        let error = parse(source).unwrap_err();
        assert!(
            error.contains(expected),
            "{error:?} did not contain {expected:?} for {source:?}"
        );
    }

    // A concrete class default is not mistaken for a forward type parameter.
    parse("<?php class Box<T = User> {}").unwrap();
}

#[cfg(any(feature = "php-generics-erased", feature = "php-generics-reified"))]
#[test]
fn declaration_variance_composes_and_rejects_wrong_polarity() {
    let valid = parse(
        r#"<?php
interface Producer<+T> { public function get(): T; }
interface Consumer<-T> { public function put(T $value); }
interface Contra<-T> {}
interface ProducerChild<+T> extends Producer<T> {}
interface ConsumerChild<-T> extends Consumer<T> {}
class CovariantBox<+T> {
    public readonly T $value;
    public function __construct(T $value) {}
    public function get(): T { return $this->value; }
    public function nested(): Contra<Contra<T>> { return null; }
}
class ReadonlyPromoted<+T> {
    public function __construct(public readonly T $value) {}
}
function transform<-I, +O>(I $input): O { return $input; }
"#,
    )
    .unwrap();
    Compiler::new().compile(&valid).unwrap();

    for (source, expected) in [
        (
            "<?php class Bad<+T> { public function take(T $value) {} }",
            "Covariant generic parameter T",
        ),
        (
            "<?php class Bad<-T> { public function get(): T { return null; } }",
            "Contravariant generic parameter T",
        ),
        (
            "<?php class Bad<+T> { public T $value; }",
            "in invariant position",
        ),
        (
            "<?php class Bad<+T> { public function __construct(public T $value) {} }",
            "in invariant position",
        ),
        (
            "<?php interface Consumer<-T> {} class Bad<+T> { public function get(): Consumer<T> { return null; } }",
            "in contravariant position",
        ),
        (
            "<?php interface Consumer<-T> {} interface Bad<+T> extends Consumer<T> {}",
            "in contravariant position",
        ),
        (
            "<?php class Wrap<T> {} class Bad<+T : Wrap<T>> {}",
            "in invariant position",
        ),
        (
            "<?php class Bad<T> { public static function take(T $value) {} }",
            "cannot be used in static context",
        ),
        (
            "<?php function bad<+T>(T $value): T { return $value; }",
            "in contravariant position",
        ),
    ] {
        let statements = parse(source).unwrap();
        let error = match Compiler::new().compile(&statements) {
            Ok(_) => panic!("expected variance error for {source:?}"),
            Err(error) => error,
        };
        assert!(
            error.contains(expected),
            "{error:?} did not contain {expected:?}"
        );
    }
}

#[cfg(any(feature = "php-generics-erased", feature = "php-generics-reified"))]
#[test]
fn parser_accepts_127_parameters_and_rejects_128() {
    fn declaration(count: usize) -> String {
        let names = (0..count)
            .map(|index| format!("T{index}"))
            .collect::<Vec<_>>()
            .join(", ");
        format!("<?php function many<{names}>() {{}}")
    }

    parse(&declaration(127)).unwrap();
    let error = parse(&declaration(128)).unwrap_err();
    assert!(error.contains("at most 127 parameters"));
}
