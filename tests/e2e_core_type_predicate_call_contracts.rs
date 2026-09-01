mod common;

use common::run_php;

const TYPE_FUNCTIONS: &str = r#"[
    'is_array', 'is_string', 'is_int', 'is_integer', 'is_long',
    'is_float', 'is_double', 'is_null', 'is_bool', 'is_numeric',
    'is_object', 'is_iterable', 'is_scalar', 'is_resource', 'gettype',
]"#;

#[test]
fn type_predicates_expose_the_php_85_standard_signatures() {
    assert_eq!(
        run_php(&format!(
            r#"<?php
foreach ({TYPE_FUNCTIONS} as $name) {{
    $function = new ReflectionFunction($name);
    $parameter = $function->getParameters()[0];
    echo $name, '|', $function->getName(), '|', $function->getExtensionName(),
        '|', $function->getNumberOfRequiredParameters(), '/',
        $function->getNumberOfParameters(), '|', $parameter->getName(), ':',
        $parameter->getType()->getName(), ':', (int) $parameter->allowsNull(),
        '|', $function->getReturnType()->getName(), ':',
        (int) $function->getReturnType()->allowsNull(), "\n";
}}
"#
        )),
        concat!(
            "is_array|is_array|standard|1/1|value:mixed:1|bool:0\n",
            "is_string|is_string|standard|1/1|value:mixed:1|bool:0\n",
            "is_int|is_int|standard|1/1|value:mixed:1|bool:0\n",
            "is_integer|is_integer|standard|1/1|value:mixed:1|bool:0\n",
            "is_long|is_long|standard|1/1|value:mixed:1|bool:0\n",
            "is_float|is_float|standard|1/1|value:mixed:1|bool:0\n",
            "is_double|is_double|standard|1/1|value:mixed:1|bool:0\n",
            "is_null|is_null|standard|1/1|value:mixed:1|bool:0\n",
            "is_bool|is_bool|standard|1/1|value:mixed:1|bool:0\n",
            "is_numeric|is_numeric|standard|1/1|value:mixed:1|bool:0\n",
            "is_object|is_object|standard|1/1|value:mixed:1|bool:0\n",
            "is_iterable|is_iterable|standard|1/1|value:mixed:1|bool:0\n",
            "is_scalar|is_scalar|standard|1/1|value:mixed:1|bool:0\n",
            "is_resource|is_resource|standard|1/1|value:mixed:1|bool:0\n",
            "gettype|gettype|standard|1/1|value:mixed:1|string:0\n",
        )
    );
}

#[test]
fn type_predicates_and_gettype_cover_every_runtime_value_family() {
    assert_eq!(
        run_php(&format!(
            r#"<?php
$open = fopen('php://temp', 'w+');
$closed = fopen('php://temp', 'w+');
fclose($closed);
$values = [
    'null' => null, 'false' => false, 'true' => true, 'int' => 1,
    'float' => 1.5, 'numeric-string' => '1.5e2', 'string' => 'x',
    'array' => [], 'object' => new stdClass(),
    'closure' => static fn () => null,
    'generator' => (static function () {{ yield 1; }})(),
    'open-resource' => $open, 'closed-resource' => $closed,
];
foreach (array_slice({TYPE_FUNCTIONS}, 0, 14) as $name) {{
    echo $name, '=';
    foreach ($values as $value) echo (int) $name($value);
    echo "\n";
}}
foreach ($values as $label => $value) echo $label, '=', gettype($value), "\n";
fclose($open);
"#
        )),
        concat!(
            "is_array=0000000100000\n",
            "is_string=0000011000000\n",
            "is_int=0001000000000\n",
            "is_integer=0001000000000\n",
            "is_long=0001000000000\n",
            "is_float=0000100000000\n",
            "is_double=0000100000000\n",
            "is_null=1000000000000\n",
            "is_bool=0110000000000\n",
            "is_numeric=0001110000000\n",
            "is_object=0000000011100\n",
            "is_iterable=0000000100100\n",
            "is_scalar=0111111000000\n",
            "is_resource=0000000000010\n",
            "null=NULL\nfalse=boolean\ntrue=boolean\nint=integer\n",
            "float=double\nnumeric-string=string\nstring=string\narray=array\n",
            "object=object\nclosure=object\ngenerator=object\n",
            "open-resource=resource\nclosed-resource=resource (closed)\n",
        )
    );
}

#[test]
fn is_numeric_uses_php_numeric_string_grammar_without_magic_conversion() {
    assert_eq!(
        run_php(
            r#"<?php
final class NumericStringable {
    public function __toString(): string { echo "unexpected magic\n"; return '1'; }
}
$values = [
    '', ' ', "\t1\n", '0', '+1', '-1', '.5', '1.', '1e2', '1e',
    '0x10', 'INF', 'NAN', '1_000', "42\0", "\xc2\xa01", "1\xc2\xa0",
    '1e309', 0, 1.5, INF, NAN, true, null, [], new NumericStringable(),
];
foreach ($values as $value) echo (int) is_numeric($value);
echo "\n";
"#,
        ),
        "00111111100000000111110000\n"
    );
}

#[test]
fn integer_and_float_aliases_keep_distinct_public_names_and_equal_results() {
    assert_eq!(
        run_php(
            r#"<?php
$values = [1, 1.0, '1', true, null];
foreach (['is_int', 'is_integer', 'is_long', 'is_float', 'is_double'] as $name) {
    echo (new ReflectionFunction($name))->getName(), ':';
    foreach ($values as $value) echo (int) $name($value);
    echo '|';
}
echo "\n";
"#,
        ),
        "is_int:10000|is_integer:10000|is_long:10000|is_float:01000|is_double:01000|\n"
    );
}

#[test]
fn iterable_and_object_predicates_distinguish_traversable_objects() {
    assert_eq!(
        run_php(
            r#"<?php
final class AggregateProbe implements IteratorAggregate {
    public function getIterator(): Traversable { yield 1; }
}
$generator = (static function () { yield 1; })();
foreach ([[], new AggregateProbe(), $generator, new stdClass(), static fn () => null] as $value) {
    echo (int) is_iterable($value), (int) is_object($value), '|';
}
echo "\n";
"#,
        ),
        "10|11|11|01|01|\n"
    );
}

#[test]
fn resource_predicates_track_open_and_closed_resource_identity() {
    assert_eq!(
        run_php(
            r#"<?php
$resource = fopen('php://temp', 'w+');
$alias =& $resource;
echo (int) is_resource($alias), ':', gettype($alias), '|';
fclose($resource);
echo (int) is_resource($alias), ':', gettype($alias), '|';
echo (int) is_scalar($alias), ':', (int) is_object($alias), "\n";
"#,
        ),
        "1:resource|0:resource (closed)|0:0\n"
    );
}

#[test]
fn type_functions_share_named_dynamic_first_class_and_callback_dispatch() {
    assert_eq!(
        run_php(
            r#"<?php
$dynamic = 'is_numeric';
$first = is_string(...);
echo (int) is_array(value: []), '|';
echo (int) $dynamic('1e2'), '|';
echo (int) $first('value'), '|';
echo (int) call_user_func('is_null', null), '|';
echo (int) call_user_func_array('is_iterable', ['value' => []]), '|';
echo implode(',', array_map('gettype', [1, 'x'])), "\n";
"#,
        ),
        "1|1|1|1|1|integer,string\n"
    );
}

#[test]
fn every_type_function_uses_direct_named_first_class_and_callback_contracts() {
    assert_eq!(
        run_php(
            r#"<?php
$resource = fopen('php://temp', 'w+');
echo 'direct=',
    (int) is_array([]),
    (int) is_string('value'),
    (int) is_int(1),
    (int) is_integer(1),
    (int) is_long(1),
    (int) is_float(1.5),
    (int) is_double(1.5),
    (int) is_null(null),
    (int) is_bool(false),
    (int) is_numeric('1e2'),
    (int) is_object(new stdClass()),
    (int) is_iterable([]),
    (int) is_scalar(1),
    (int) is_resource($resource),
    (int) (gettype(1) === 'integer'), "\n";
$cases = [
    'is_array' => [[], true],
    'is_string' => ['value', true],
    'is_int' => [1, true],
    'is_integer' => [1, true],
    'is_long' => [1, true],
    'is_float' => [1.5, true],
    'is_double' => [1.5, true],
    'is_null' => [null, true],
    'is_bool' => [false, true],
    'is_numeric' => ['1e2', true],
    'is_object' => [new stdClass(), true],
    'is_iterable' => [[], true],
    'is_scalar' => [1, true],
    'is_resource' => [$resource, true],
    'gettype' => [1, 'integer'],
];
foreach ($cases as $name => [$value, $expected]) {
    $firstClass = $name(...);
    echo $name, '=',
        (int) ($name(value: $value) === $expected),
        (int) ($firstClass(value: $value) === $expected),
        (int) (call_user_func_array($name, ['value' => $value]) === $expected),
        "\n";
}
fclose($resource);
"#,
        ),
        concat!(
            "direct=111111111111111\n",
            "is_array=111\n",
            "is_string=111\n",
            "is_int=111\n",
            "is_integer=111\n",
            "is_long=111\n",
            "is_float=111\n",
            "is_double=111\n",
            "is_null=111\n",
            "is_bool=111\n",
            "is_numeric=111\n",
            "is_object=111\n",
            "is_iterable=111\n",
            "is_scalar=111\n",
            "is_resource=111\n",
            "gettype=111\n",
        )
    );
}

#[test]
fn type_function_registration_owns_arity_and_named_argument_errors() {
    assert_eq!(
        run_php(
            r#"<?php
foreach ([
    static fn () => is_array(),
    static fn () => gettype(null, null),
    static fn () => is_bool(nope: true),
] as $call) {
    try { $call(); }
    catch (Throwable $error) { echo $error::class, ':', $error->getMessage(), "\n"; }
}
"#,
        ),
        concat!(
            "ArgumentCountError:is_array() expects exactly 1 argument, 0 given\n",
            "ArgumentCountError:gettype() expects exactly 1 argument, 2 given\n",
            "Error:Unknown named parameter $nope\n",
        )
    );
}

#[test]
fn type_predicates_read_references_without_coercion_or_detachment() {
    assert_eq!(
        run_php(
            r#"<?php
final class PredicateStringable {
    public function __toString(): string { echo "unexpected magic\n"; return '123'; }
}
$value = '123';
$alias =& $value;
echo (int) is_numeric($alias), (int) is_string($alias), '|', $value, ':', $alias, '|';
$value[0] = 'x';
echo $value, ':', $alias, '|';
$object = new PredicateStringable();
echo (int) is_object($object), (int) is_scalar($object), (int) is_numeric($object),
    ':', gettype($object), "\n";
"#,
        ),
        "11|123:123|x23:x23|100:object\n"
    );
}

#[test]
fn type_function_inventory_is_case_insensitive_namespaced_and_strict_safe() {
    assert_eq!(
        run_php(&format!(
            r#"<?php declare(strict_types=1);
namespace TypePredicateProbe;
$reflection = new \ReflectionFunction('IS_INTEGER');
echo (int) \function_exists('IS_INTEGER'), ':', $reflection->getName(), ':',
    $reflection->getExtensionName(), ':', (int) is_array([]), ':',
    (int) \IS_LONG(1), ':', gettype(value: 'x'), '|';
$internal = \get_defined_functions()['internal'];
foreach ({TYPE_FUNCTIONS} as $name) echo (int) \in_array($name, $internal, true);
echo "\n";
"#
        )),
        "1:is_integer:standard:1:1:string|111111111111111\n"
    );
}
