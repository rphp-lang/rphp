mod common;
use common::run_php;

#[test]
fn magic_calls_pack_named_arguments_without_losing_order_or_identity() {
    let output = run_php(
        r#"<?php
class NamedMagic {
    public function __call($name, $arguments) {
        echo "instance:$name:", json_encode($arguments), "\n";
        if ($name === 'MiXeD') {
            $arguments['array'][] = 'inside';
            $arguments['object']->count++;
        }
    }

    public static function __callStatic($name, $arguments) {
        echo "static:$name:", json_encode($arguments), "\n";
    }
}

function ordered($name, $value) {
    echo "eval:$name\n";
    return $value;
}

$array = ['outside'];
$object = (object) ['count' => 1];
$magic = new NamedMagic;
$magic->MiXeD(
    ordered('positional', 10),
    named: ordered('named', 20),
    array: $array,
    object: $object,
);
echo 'state:', json_encode($array), ':', $object->count, "\n";

NamedMagic::StaTiC(...[30, 'named' => ordered('static-named', 40)]);

$instance = $magic->Through(...);
$static = NamedMagic::StaticThrough(...);
$instance(1, 2, 3, first: 4, second: $object);
$static(...['first' => 4, 'second' => (object) []]);
"#,
    );

    assert_eq!(
        output,
        "eval:positional\n\
eval:named\n\
instance:MiXeD:{\"0\":10,\"named\":20,\"array\":[\"outside\"],\"object\":{\"count\":1}}\n\
state:[\"outside\"]:2\n\
eval:static-named\n\
static:StaTiC:{\"0\":30,\"named\":40}\n\
instance:Through:{\"0\":1,\"1\":2,\"2\":3,\"first\":4,\"second\":{\"count\":2}}\n\
static:StaticThrough:{\"first\":4,\"second\":{}}\n"
    );
}

#[test]
fn magic_named_argument_errors_preserve_state_and_ordinary_signatures() {
    let output = run_php(
        r#"<?php
class GuardedMagic {
    public function __call($name, $arguments) {
        if ($name === 'Reference') {
            $arguments['value'] = 9;
        }
        echo "dispatch:$name\n";
    }
}

function spread($label, $value) {
    echo "eval:$label\n";
    return ['same' => $value];
}

$number = 7;
$arguments = ['value' => &$number];
$magic = new GuardedMagic;
$magic->Reference(...$arguments);
echo "reference:$number:", $arguments['value'], "\n";

try {
    $magic->Duplicate(...spread('first', 1), ...spread('second', 2));
} catch (Throwable $error) {
    echo 'duplicate:', get_class($error), ':', $error->getMessage(), "\n";
}

function callTemporary(Closure $closure) {
    try {
        $closure(wanted: 1);
    } catch (Throwable $error) {
        echo 'ordinary:', get_class($error), ':', $error->getMessage(), "\n";
    }
}
callTemporary(function($wanted) { echo "ordinary:first:$wanted\n"; });
callTemporary(function($other) { echo "ordinary:unexpected\n"; });

try {
    $magic->Stopped(value: (function() {
        echo "eval:throwing\n";
        throw new Exception('argument-stop');
    })(), later: print('unexpected'));
} catch (Throwable $error) {
    echo 'stopped:', get_class($error), ':', $error->getMessage(), "\n";
}
"#,
    );

    assert_eq!(
        output,
        "dispatch:Reference\n\
reference:7:7\n\
eval:first\n\
eval:second\n\
duplicate:Error:Named parameter $same overwrites previous argument\n\
ordinary:first:1\n\
ordinary:Error:Unknown named parameter $wanted\n\
eval:throwing\n\
stopped:Exception:argument-stop\n"
    );
}

#[test]
fn magic_first_class_callables_expose_their_public_trampoline_signature() {
    assert_eq!(
        run_php(
            r#"<?php
class ReflectedMagic {
    public function __call($name, $arguments) {}
    public static function __callStatic($name, $arguments) {}
}

foreach ([(new ReflectedMagic)->missing(...), ReflectedMagic::missingStatic(...)] as $closure) {
    $reflection = new ReflectionFunction($closure);
    $parameters = $reflection->getParameters();
    $parameter = $parameters[0];
    echo $reflection->getNumberOfRequiredParameters(), '/',
        $reflection->getNumberOfParameters(), ':', count($parameters), ':',
        $parameter->getName(), ':', (int) $parameter->isVariadic(), ':',
        $parameter->getType()->getName(), "\n";
}
"#,
        ),
        "0/1:1:arguments:1:mixed\n0/1:1:arguments:1:mixed\n"
    );
}
