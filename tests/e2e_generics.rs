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
echo id::<int>("accepted by mixed erasure");
echo wrong::<int>();
"#,
    );
    assert_eq!(output, "accepted by mixed erasurestill erased");
}

#[cfg(all(feature = "php-generics-erased", not(feature = "php-generics-reified")))]
#[test]
fn erased_generic_properties_keep_the_erased_storage_contract() {
    let output = common::run_php(
        r#"<?php
class Box<T> { public T $value; }
$box = new Box::<int>();
$box->value = "accepted by mixed erasure";
echo $box->value;
$reflection = new ReflectionObject($box);
echo ":" . count($reflection->getGenericArguments());
echo ":" . count($reflection->getGenericParameters());
"#,
    );
    assert_eq!(output, "accepted by mixed erasure:0:1");

    let error = common::run_php_expect_error(
        r#"<?php
class IntBox<T : int> { public T $value; }
$box = new IntBox::<int>();
$box->value = "not an int";
"#,
    );
    assert!(format!("{error:?}").contains("bound-erased property IntBox::$value"));
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
$ints = new Box::<int>();
$strings = new Box::<string>();
$ints->value = 1;
$strings->value = "s";
$clone = clone $ints;
$clone->value = 2;
echo $ints->value . $strings->value . $clone->value;
$intArguments = (new ReflectionObject($ints))->getGenericArguments();
$stringArguments = (new ReflectionObject($strings))->getGenericArguments();
echo ":" . $intArguments[0] . ":" . $stringArguments[0];
function assignBox($box, $value) { $box->value = $value; }
assignBox($ints, 3);
assignBox($strings, "t");
echo ":" . $ints->value . ":" . $strings->value;
"#,
    );
    assert_eq!(output, "1s2:int:string:3:t");

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
        format!("{promoted_error:?}").contains("reified property Promoted::$value"),
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

#[cfg(any(feature = "php-generics-erased", feature = "php-generics-reified"))]
#[test]
fn compiler_preserves_interned_pre_erasure_metadata_off_hot_structures() {
    let statements = parse(
        r#"<?php
interface Source<+T> {}
trait Holder<T : object> {}
class Box<T : object = stdClass> {
    public T $value;
    public function pair<-L, +R : Box<T>>(L $left, R $right): Box<R> { return $right; }
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
    public function map<+U : string>(U $value): U { return $value; }
}
$function = new ReflectionFunction("id");
$functionParameters = $function->getGenericParameters();
echo $function->isGeneric() ? "yes:" : "no:";
echo $functionParameters[0]["name"] . ":" . $functionParameters[0]["bound"] . ":";
$class = new ReflectionClass("Box");
$classParameters = $class->getGenericParameters();
echo $classParameters[0]["default"] . ":";
$method = new ReflectionMethod("Box", "map");
$methodParameters = $method->getGenericParameters();
echo $methodParameters[0]["variance"] . ":";
echo count($method->getGenericRuntimeModes());
"#,
    );
    let mode_count = usize::from(cfg!(feature = "php-generics-erased"))
        + usize::from(cfg!(feature = "php-generics-reified"));
    assert_eq!(output, format!("yes:T:int:stdClass:covariant:{mode_count}"));
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
