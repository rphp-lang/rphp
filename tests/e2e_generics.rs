mod common;

#[cfg(any(feature = "php-generics-erased", feature = "php-generics-reified"))]
use rphp::compiler::compile::Compiler;
#[cfg(any(feature = "php-generics-erased", feature = "php-generics-reified"))]
use rphp::generics::{GenericDeclarationKind, GenericType, GenericVariance};
use rphp::lexer::Lexer;
use rphp::parser::Parser;
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

    let use_error = parse("<?php id::<int>(1);").unwrap_err();
    assert_eq!(
        use_error,
        "Generic syntax requires php-generics-erased or php-generics-reified"
    );

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
"#,
    );
    assert_eq!(output, "no00");
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

#[cfg(feature = "php-generics-reified")]
#[test]
fn reified_substitution_that_differs_from_erasure_keeps_boundary_checks() {
    let statements =
        parse("<?php function id<T>(T $value): T { return $value; } echo id::<int>(1);").unwrap();
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
            "missing {opcode:?}"
        );
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
    assert_eq!(boxed.methods.len(), 1);
    assert_eq!(boxed.methods[0].value_parameters.len(), 1);

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
    ];
    for (source, expected) in cases {
        let error = parse(source).unwrap_err();
        assert!(
            error.contains(expected),
            "{error:?} did not contain {expected:?}"
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
