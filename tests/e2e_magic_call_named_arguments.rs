mod common;
use common::run_php;

#[test]
fn inaccessible_methods_select_magic_trampolines_before_visibility_errors() {
    assert_eq!(
        run_php(
            r#"<?php
class InstanceBase {
    private function MiXeD(int $value): string { return "base:$value"; }
}
class InstanceChild extends InstanceBase {
    public function __call(string $name, array $arguments): mixed {
        echo "instance:$name:", json_encode($arguments), "|";
        return "magic";
    }
}
$value = 7;
var_dump((new InstanceChild)->MiXeD($value, label: 'named'));
echo "value:$value|";

class ProtectedTarget {
    protected function secret(string $value): void { echo "secret:$value|"; }
    public function __call(string $name, array $arguments): mixed {
        echo "protected:$name:", implode(',', $arguments), "|";
        return 42;
    }
}
var_dump((new ProtectedTarget)->secret('payload'));

class StaticTarget {
    private static function HiDdEn(int $value): void { echo "hidden:$value|"; }
    public static function __callStatic(string $name, array $arguments): mixed {
        echo "static:$name:", json_encode($arguments), "|";
        return 'result';
    }
}
var_dump(StaticTarget::HiDdEn(3, mode: 'named'));
$staticObject = new StaticTarget();
var_dump($staticObject::DyNaMiC(4));
var_dump(statictarget::CaSeD(5));

class InvalidStaticContext {
    public static function __callStatic(string $name, array $arguments): mixed {
        try {
            var_dump($this);
        } catch (Error $error) {
            echo $error->getMessage(), ":$name|";
        }
        return null;
    }
}
InvalidStaticContext::Missing();

class NoMagic {
    private function hidden(): void {}
    protected static function guarded(): void {}
}
$object = new NoMagic;
foreach ([fn() => $object->hidden(), fn() => NoMagic::guarded()] as $call) {
    try {
        $call();
    } catch (Error $error) {
        echo $error->getMessage(), "|";
    }
}
"#,
        ),
        concat!(
            "instance:MiXeD:{\"0\":7,\"label\":\"named\"}|string(5) \"magic\"\n",
            "value:7|protected:secret:payload|int(42)\n",
            "static:HiDdEn:{\"0\":3,\"mode\":\"named\"}|string(6) \"result\"\n",
            "static:DyNaMiC:[4]|string(6) \"result\"\n",
            "static:CaSeD:[5]|string(6) \"result\"\n",
            "Using $this when not in object context:Missing|",
            "Call to private method NoMagic::hidden() from global scope|",
            "Call to protected method NoMagic::guarded() from global scope|",
        )
    );
}

#[test]
fn scoped_instance_and_abstract_static_magic_keep_php_dispatch_identity() {
    assert_eq!(
        run_php(
            r#"<?php
class ScopedBase {
    private function hidden(): void {}
    public function __call(string $name, array $arguments): mixed {
        echo "base-call:$name:", implode(',', $arguments), "|";
        return $name;
    }
}
class ScopedChild extends ScopedBase {
    public function run(): void {
        var_dump(self::SelfCase(1));
        var_dump(parent::hidden());
        var_dump(static::StaticCase(2));
        var_dump(ScopedChild::ClassCase(3));
    }
}
(new ScopedChild)->run();

interface AbstractInterface {
    public static function __callStatic($name, $arguments);
}
abstract class AbstractClass {
    abstract public static function __callStatic($name, $arguments);
}
foreach ([AbstractInterface::class, AbstractClass::class] as $class) {
    try {
        $class::missing();
    } catch (Error $error) {
        echo $error->getMessage(), "|";
    }
}
"#,
        ),
        concat!(
            "base-call:SelfCase:1|string(8) \"SelfCase\"\n",
            "base-call:hidden:|string(6) \"hidden\"\n",
            "base-call:StaticCase:2|string(10) \"StaticCase\"\n",
            "base-call:ClassCase:3|string(9) \"ClassCase\"\n",
            "Cannot call abstract method AbstractInterface::missing()|",
            "Cannot call abstract method AbstractClass::missing()|",
        )
    );
}

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
