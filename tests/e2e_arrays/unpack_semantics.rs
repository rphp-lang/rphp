#[test]
fn array_unpack_consumes_iterator_aggregates_with_php_key_rules() {
    assert_eq!(
        run_php(
            r#"<?php
class UnpackFeed implements IteratorAggregate {
    public function getIterator(): Traversable {
        yield 40 => 'iter-int';
        yield '12' => 'numeric-string';
        yield 'label' => 'iter-label';
    }
}

$result = [
    'seed' => 'kept',
    ...[90 => 'array-int', 'label' => 'array-label'],
    ...new UnpackFeed(),
    'tail',
];
foreach ($result as $key => $value) {
    echo $key, '=', $value, ';';
}
"#,
        ),
        "seed=kept;0=array-int;label=iter-label;1=iter-int;2=numeric-string;3=tail;"
    );
}

#[test]
fn array_unpack_rejects_invalid_traversable_keys_as_a_catchable_error() {
    assert_eq!(
        run_php(
            r#"<?php
function invalidUnpackKey() {
    yield [] => 'unreachable';
}

try {
    $result = [...invalidUnpackKey()];
} catch (Error $error) {
    echo get_class($error), ':', $error->getMessage();
}
"#,
        ),
        "Error:Keys must be of type int|string during array unpacking"
    );
}

#[test]
fn array_unpack_rejects_non_iterables_as_a_catchable_error() {
    assert_eq!(
        run_php(
            r#"<?php
$source = 17;
try {
    $result = [...$source];
} catch (Error $error) {
    echo get_class($error), ':', $error->getMessage();
}
"#,
        ),
        "Error:Only arrays and Traversables can be unpacked"
    );
}

#[test]
fn statically_known_scalar_unpack_operands_fail_during_compilation() {
    for (source, line) in [
        ("<?php\n$result = [\n    ...(20 + 22),\n];", 3),
        ("<?php\n$result = [\n    ...__LINE__,\n];", 3),
        (
            "<?php\nclass StaticSpreadOperand { const VALUE = 42; }\n$result = [\n    ...StaticSpreadOperand::VALUE,\n];",
            4,
        ),
    ] {
        let tokens = rphp::lexer::Lexer::new(source).tokenize().unwrap();
        let statements = rphp::parser::Parser::new(tokens).parse().unwrap();
        let error = match rphp::compiler::compile::Compiler::new()
            .with_source_context("/fixture/static-spread.php", "/fixture")
            .compile(&statements)
        {
            Ok(_) => panic!("a statically known scalar spread must not compile"),
            Err(error) => error,
        };

        assert_eq!(
            error,
            format!(
                "Only arrays and Traversables can be unpacked in /fixture/static-spread.php on line {line}"
            )
        );
    }
}

#[test]
fn runtime_scalar_sources_remain_catchable() {
    assert_eq!(
        run_php(
            r#"<?php
const RUNTIME_SPREAD_SOURCE = 17;
$constructed = new stdClass();
foreach ([RUNTIME_SPREAD_SOURCE, $constructed] as $source) {
    try {
        $result = [...$source];
    } catch (Error $error) {
        echo get_class($error), ':', $error->getMessage(), "\n";
    }
}
"#,
        ),
        "Error:Only arrays and Traversables can be unpacked\nError:Only arrays and Traversables can be unpacked\n"
    );
}

#[test]
fn array_unpack_dereferences_source_elements_by_value() {
    assert_eq!(
        run_php(
            "<?php $number = 4; $source = [&$number]; $copy = [...$source]; $number = 8; echo $copy[0], ':', $source[0];"
        ),
        "4:8"
    );
}

#[test]
fn array_unpack_reports_exhausted_integer_key_space_without_wrapping() {
    assert_eq!(
        run_php(
            r#"<?php
try {
    $result = [PHP_INT_MAX - 2 => 'edge', ...['first', 'second', 'overflow']];
} catch (Error $error) {
    echo get_class($error), ':', $error->getMessage();
}
"#,
        ),
        "Error:Cannot add element to the array as the next element is already occupied"
    );
}
