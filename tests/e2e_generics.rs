mod common;

#[cfg(any(feature = "php-generics-erased", feature = "php-generics-reified"))]
use rphp::compiler::compile::Compiler;
#[cfg(any(feature = "php-generics-erased", feature = "php-generics-reified"))]
use rphp::generics::{GenericDeclarationKind, GenericType, GenericVariance};
use rphp::lexer::Lexer;
use rphp::parser::Parser;

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
fn compiler_preserves_interned_pre_erasure_metadata_off_hot_structures() {
    let statements = parse(
        r#"<?php
interface Source<+T> {}
trait Holder<T : object> {}
class Box<T : object = stdClass> {
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
