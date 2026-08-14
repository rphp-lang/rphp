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
