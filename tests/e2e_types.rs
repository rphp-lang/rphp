/// E2E tests: isset, empty, unset, type casting, type checks.
mod common;
use common::run_php;

#[test]
fn error_reporting_is_request_local_and_uses_namespaced_function_fallback() {
    assert_eq!(
        run_php(
            "<?php namespace App; echo error_reporting(), ':'; echo error_reporting(5), ':'; echo error_reporting();"
        ),
        "32767:32767:5"
    );
}

#[test]
fn error_log_default_destination_uses_namespaced_function_fallback() {
    assert_eq!(
        run_php("<?php namespace App; var_dump(error_log('rphp-test'));"),
        "bool(true)\n"
    );
}

#[test]
fn invokable_object_satisfies_callable_parameter_and_return_types() {
    assert_eq!(
        run_php(
            "<?php class Handler { public function __invoke(): string { return 'OK'; } } function keep(callable $handler): callable { return $handler; } echo keep(new Handler())();"
        ),
        "OK"
    );
}

#[test]
fn reflection_function_exposes_anonymous_and_bound_closure_identity() {
    assert_eq!(
        run_php(
            "<?php $anonymous = function () {}; $anonymousReflection = new ReflectionFunction($anonymous); var_dump($anonymousReflection->isAnonymous()); echo count($anonymousReflection->getAttributes()), ':'; class Bound { public function named() {} public function callback() { return $this->named(...); } } $bound = new Bound(); $reflection = new ReflectionFunction($bound->callback()); var_dump($reflection->isAnonymous()); echo ($reflection->getClosureThis() === $bound ? 'bound:' : 'missing:') . $reflection->getClosureCalledClass()->name;"
        ),
        "bool(true)\n0:bool(false)\nbound:Bound"
    );
}

#[test]
fn reflection_function_parameters_expose_controller_metadata_surface() {
    assert_eq!(
        run_php(
            "<?php function reflected(string $name, ?int $count = null, bool ...$flags) {} $parameters = (new ReflectionFunction('reflected'))->getParameters(); foreach ($parameters as $parameter) { $type = $parameter->getType(); echo $parameter->getName(), ':', $type?->getName(), ':', (int) $type?->isBuiltin(), ':', (int) $parameter->allowsNull(), ':', (int) $parameter->isDefaultValueAvailable(), ':', (int) $parameter->isVariadic(), '|'; }"
        ),
        "name:string:1:0:0:0|count:int:1:1:1:0|flags:bool:1:0:0:1|"
    );
}

#[test]
fn reflection_class_lazy_ghost_eagerly_runs_initializer_on_real_instance() {
    assert_eq!(
        run_php(
            "<?php class LazyService { public string $value; public function __construct(string $value) { $this->value = $value; } } $service = (new ReflectionClass(LazyService::class))->newLazyGhost(static function ($ghost) { $ghost->__construct('OK'); }); echo $service->value;"
        ),
        "OK"
    );
}

#[test]
fn core_iterator_and_collection_interfaces_are_registered() {
    assert_eq!(
        run_php(
            "<?php echo interface_exists('IteratorAggregate', false) ? 'yes' : 'no'; echo ':'; echo interface_exists('ArrayAccess', false) ? 'yes' : 'no';"
        ),
        "yes:yes"
    );
}

#[test]
fn spl_object_storage_is_a_core_collection_type() {
    assert_eq!(
        run_php(
            "<?php $storage = new SplObjectStorage(); echo $storage instanceof Iterator ? 'iterator:' : 'missing:'; echo $storage instanceof ArrayAccess ? 'array-access' : 'missing';"
        ),
        "iterator:array-access"
    );
}

#[test]
fn array_iterator_participates_in_foreach() {
    assert_eq!(
        run_php(
            "<?php $iterator = new ArrayIterator(['first' => 1, 'second' => 2]); foreach ($iterator as $key => $value) echo $key . ':' . $value . '|';"
        ),
        "first:1|second:2|"
    );
}

#[test]
fn array_iterator_and_array_object_expose_distinct_traversal_contracts() {
    assert_eq!(
        run_php(
            "<?php $iterator = new ArrayIterator([]); $object = new ArrayObject([]); echo (int) ($iterator instanceof Iterator), (int) ($iterator instanceof IteratorAggregate), '|', (int) ($object instanceof Iterator), (int) ($object instanceof IteratorAggregate);"
        ),
        "10|01"
    );
}

#[test]
fn error_and_exception_handler_stacks_restore_previous_callbacks() {
    assert_eq!(
        run_php(
            "<?php set_error_handler('strlen'); echo get_error_handler(), ':'; set_error_handler(null); restore_error_handler(); echo get_error_handler(), ':'; set_exception_handler('trim'); echo get_exception_handler();"
        ),
        "strlen:strlen:trim"
    );
}

// ========== isset() ==========

#[test]
fn test_isset_defined_var() {
    assert_eq!(
        run_php("<?php $x = 42; echo isset($x) ? 'yes' : 'no';"),
        "yes"
    );
}

#[test]
fn test_isset_undefined_var() {
    assert_eq!(run_php("<?php echo isset($x) ? 'yes' : 'no';"), "no");
}

#[test]
fn test_isset_null_var() {
    assert_eq!(
        run_php("<?php $x = null; echo isset($x) ? 'yes' : 'no';"),
        "no"
    );
}

#[test]
fn test_isset_zero_is_set() {
    assert_eq!(
        run_php("<?php $x = 0; echo isset($x) ? 'yes' : 'no';"),
        "yes"
    );
}

#[test]
fn test_isset_empty_string_is_set() {
    assert_eq!(
        run_php("<?php $x = ''; echo isset($x) ? 'yes' : 'no';"),
        "yes"
    );
}

#[test]
fn test_isset_false_is_set() {
    assert_eq!(
        run_php("<?php $x = false; echo isset($x) ? 'yes' : 'no';"),
        "yes"
    );
}

#[test]
fn test_isset_multi_arg() {
    assert_eq!(
        run_php("<?php $a = 1; $b = 2; echo isset($a, $b) ? 'yes' : 'no';"),
        "yes"
    );
}

#[test]
fn test_isset_multi_arg_one_null() {
    assert_eq!(
        run_php("<?php $a = 1; $b = null; echo isset($a, $b) ? 'yes' : 'no';"),
        "no"
    );
}

#[test]
fn test_isset_rejects_expression() {
    let result =
        std::panic::catch_unwind(|| run_php("<?php $x = 1; echo isset($x + 1) ? 'yes' : 'no';"));
    assert!(result.is_err());
}

#[test]
fn test_isset_accepts_property_chains() {
    assert_eq!(
        run_php(
            "<?php
            class Boxed { public $value; }
            $null = null;
            $object = new Boxed();
            $object->value = new Boxed();
            $object->value->value = 42;
            var_dump(isset($null->missing->nested));
            var_dump(isset($object->value->value));
            var_dump(isset($object->value->missing));
            var_dump(isset($null->missing['key']->nested));
            "
        ),
        "bool(false)\nbool(true)\nbool(false)\nbool(false)\n"
    );
}

#[test]
fn test_multi_isset_short_circuits_property_magic() {
    assert_eq!(
        run_php(
            "<?php
            class MagicProbe {
                public function __isset($name) {
                    echo 'unexpected';
                    return true;
                }
            }
            $missing = null;
            $probe = new MagicProbe();
            var_dump(isset($missing, $probe->virtual));
            "
        ),
        "bool(false)\n"
    );
}

#[test]
fn test_chained_isset_calls_magic_in_php_order() {
    assert_eq!(
        run_php(
            "<?php
            class MagicChain {
                public function __isset($name) {
                    echo 'isset:' . $name . \"\\n\";
                    return $name !== 'missing';
                }
                public function __get($name) {
                    echo 'get:' . $name . \"\\n\";
                    return new MagicChain();
                }
            }
            $object = new MagicChain();
            var_dump(isset($object->first->second));
            var_dump(isset($object->missing->second));
            "
        ),
        "isset:first\nget:first\nisset:second\nbool(true)\nisset:missing\nbool(false)\n"
    );
}

#[test]
fn test_isset_object_property_uses_magic_only_when_unresolved() {
    assert_eq!(
        run_php(
            "<?php
            class MagicBox {
                public $present = null;
                public function __isset($name) {
                    echo 'magic:' . $name . \"\\n\";
                    return $name === 'virtual';
                }
            }
            $object = new MagicBox();
            var_dump(isset($object->present));
            var_dump(isset($object->virtual));
            var_dump(isset($object->missing));
            "
        ),
        "bool(false)\nmagic:virtual\nbool(true)\nmagic:missing\nbool(false)\n"
    );
}

#[test]
fn test_isset_inaccessible_property_does_not_leak_and_magic_exceptions_propagate() {
    assert_eq!(
        run_php(
            "<?php
            class HiddenBox {
                private $hiddenValue = 42;
                public function __isset($name) {
                    if ($name === 'boom') { throw new Exception('isset failed'); }
                    echo 'magic:' . $name . \"\\n\";
                    return false;
                }
                public function __get($name) { echo 'leaked'; return $this->hiddenValue; }
            }
            $object = new HiddenBox();
            var_dump(isset($object->hiddenValue->nested));
            try { isset($object->boom); } catch (Exception $error) {
                echo $error->getMessage() . \"\\n\";
            }
            "
        ),
        "magic:hiddenValue\nbool(false)\nisset failed\n"
    );
}

// ========== empty() ==========

#[test]
fn test_empty_undefined() {
    assert_eq!(run_php("<?php echo empty($x) ? 'yes' : 'no';"), "yes");
}

#[test]
fn test_empty_null() {
    assert_eq!(
        run_php("<?php $x = null; echo empty($x) ? 'yes' : 'no';"),
        "yes"
    );
}

#[test]
fn test_empty_false() {
    assert_eq!(
        run_php("<?php $x = false; echo empty($x) ? 'yes' : 'no';"),
        "yes"
    );
}

#[test]
fn test_empty_zero() {
    assert_eq!(
        run_php("<?php $x = 0; echo empty($x) ? 'yes' : 'no';"),
        "yes"
    );
}

#[test]
fn test_empty_empty_string() {
    assert_eq!(
        run_php("<?php $x = ''; echo empty($x) ? 'yes' : 'no';"),
        "yes"
    );
}

#[test]
fn test_empty_nonempty() {
    assert_eq!(
        run_php("<?php $x = 'hello'; echo empty($x) ? 'yes' : 'no';"),
        "no"
    );
}

#[test]
fn test_empty_array_empty() {
    assert_eq!(
        run_php("<?php $x = []; echo empty($x) ? 'yes' : 'no';"),
        "yes"
    );
}

#[test]
fn test_empty_array_nonempty() {
    assert_eq!(
        run_php("<?php $x = [1]; echo empty($x) ? 'yes' : 'no';"),
        "no"
    );
}

#[test]
fn empty_silently_probes_uninitialized_typed_property_dimensions() {
    assert_eq!(
        run_php(
            r#"<?php
class EmptyTypedPropertyProbe {
    private array $values;
    public function missing(string $key): bool {
        return empty($this->values[$key]);
    }
    public function initialize(): void {
        $this->values = ['zero' => 0, 'value' => 4];
    }
}
class TypedArrayDimensionInitializer {
    private array $values;
    public function set(string $key, int $value): void {
        $this->values[$key] = $value;
    }
    public function get(string $key): int {
        return $this->values[$key];
    }
}
$probe = new EmptyTypedPropertyProbe();
echo $probe->missing('unset') ? 'silent:' : 'bad:';
$probe->initialize();
echo $probe->missing('zero') ? 'empty:' : 'bad:';
echo $probe->missing('value') ? 'bad:' : 'value:';
$initializer = new TypedArrayDimensionInitializer();
$initializer->set('created', 9);
echo $initializer->get('created');
"#,
        ),
        "silent:empty:value:9"
    );
}

// ========== unset() ==========

#[test]
fn test_unset_basic() {
    assert_eq!(
        run_php("<?php $x = 42; unset($x); echo isset($x) ? 'yes' : 'no';"),
        "no"
    );
}

#[test]
fn test_unset_multiple() {
    assert_eq!(
        run_php("<?php $a = 1; $b = 2; unset($a, $b); echo isset($a) ? 'yes' : 'no';"),
        "no"
    );
}

#[test]
fn test_unset_then_reassign() {
    assert_eq!(run_php("<?php $x = 1; unset($x); $x = 2; echo $x;"), "2");
}

#[test]
fn test_unset_array_element() {
    assert_eq!(
        run_php("<?php $a = [10, 20, 30]; unset($a[1]); echo count($a);"),
        "2"
    );
}

#[test]
fn test_unset_array_element_isset() {
    assert_eq!(
        run_php(
            "<?php $a = ['x' => 1, 'y' => 2]; unset($a['x']); echo isset($a['x']) ? 'yes' : 'no';"
        ),
        "no"
    );
}

#[test]
fn test_unset_array_preserves_other() {
    assert_eq!(
        run_php("<?php $a = [1, 2, 3]; unset($a[0]); echo $a[1] . $a[2];"),
        "23"
    );
}

#[test]
fn test_unset_dynamic_object_property() {
    assert_eq!(
        run_php(
            "<?php $state = (object)['route' => 'home', 'keep' => 1]; unset($state->route); echo (isset($state->route) ? 'set' : 'unset') . ':' . $state->keep;"
        ),
        "unset:1"
    );
}

#[test]
fn test_unset_declared_object_property() {
    assert_eq!(
        run_php(
            "<?php class Box { public $value = 1; } $box = new Box(); unset($box->value); echo isset($box->value) ? 'set' : 'unset';"
        ),
        "unset"
    );
}

// ========== Type casting ==========

#[test]
fn test_cast_int_from_float() {
    assert_eq!(run_php("<?php echo (int)3.7;"), "3");
}

#[test]
fn test_cast_int_from_string() {
    assert_eq!(run_php("<?php echo (int)'42abc';"), "42");
}

#[test]
fn test_cast_int_from_bool_true() {
    assert_eq!(run_php("<?php echo (int)true;"), "1");
}

#[test]
fn test_cast_int_from_bool_false() {
    assert_eq!(run_php("<?php echo (int)false;"), "0");
}

#[test]
fn test_cast_int_from_null() {
    assert_eq!(run_php("<?php echo (int)null;"), "0");
}

#[test]
fn test_cast_float_from_int() {
    assert_eq!(run_php("<?php $x = (float)42; echo $x + 0.5;"), "42.5");
}

#[test]
fn test_cast_float_from_string() {
    assert_eq!(run_php("<?php echo (float)'3.14';"), "3.14");
}

#[test]
fn test_cast_string_from_int() {
    assert_eq!(run_php("<?php $s = (string)42; echo strlen($s);"), "2");
}

#[test]
fn test_cast_string_from_float() {
    assert_eq!(run_php("<?php $s = (string)3.14; echo $s;"), "3.14");
}

#[test]
fn test_cast_string_from_bool() {
    assert_eq!(run_php("<?php echo (string)true;"), "1");
}

#[test]
fn test_cast_bool_truthy() {
    assert_eq!(run_php("<?php echo (bool)42 ? 'yes' : 'no';"), "yes");
}

#[test]
fn test_cast_bool_falsy() {
    assert_eq!(run_php("<?php echo (bool)0 ? 'yes' : 'no';"), "no");
}

#[test]
fn test_cast_array_from_scalar() {
    assert_eq!(
        run_php("<?php $a = (array)42; echo count($a) . ':' . $a[0];"),
        "1:42"
    );
}

#[test]
fn test_cast_array_from_null() {
    assert_eq!(run_php("<?php $a = (array)null; echo count($a);"), "0");
}

#[test]
fn test_cast_array_from_array() {
    assert_eq!(
        run_php("<?php $a = [1,2]; $b = (array)$a; echo count($b);"),
        "2"
    );
}

#[test]
fn object_to_array_cast_projects_properties_instead_of_wrapping_the_object() {
    assert_eq!(
        run_php(
            "<?php class CastNode { public $payload = ['ok']; } $cast = (array) new CastNode(); echo count($cast), ':', $cast['payload'][0];"
        ),
        "1:ok"
    );
}

#[test]
fn object_to_array_cast_mangles_visibility_and_skips_uninitialized_slots() {
    assert_eq!(
        run_php(
            "<?php class CastBox { public $pub = 1; protected $prot = 2; private $priv = 3; public int $unset; } $box = new CastBox(); $box->dyn = 4; $keys = ['pub', \"\0*\0prot\", \"\0CastBox\0priv\", 'dyn']; $index = 0; $ok = true; foreach ((array) $box as $key => $value) { $ok = $ok && $key === $keys[$index] && $value === $index + 1; ++$index; } echo $ok ? 'OK:' : 'BAD:', $index;"
        ),
        "OK:4"
    );
}

#[test]
fn test_cast_object_from_array_scalar_and_null() {
    assert_eq!(
        run_php(
            "<?php $a = (object)['name' => 'route', 0 => 'zero']; $s = (object)42; $n = (object)null; echo $a->name . ':' . $s->scalar . ':' . (isset($n->missing) ? 'set' : 'empty');"
        ),
        "route:42:empty"
    );
}

#[test]
fn test_cast_object_keeps_object_identity() {
    assert_eq!(
        run_php(
            "<?php $a = (object)['value' => 1]; $b = (object)$a; $b->value = 2; echo $a->value;"
        ),
        "2"
    );
}

#[test]
fn test_cast_integer_keyword() {
    assert_eq!(run_php("<?php echo (integer)3.7;"), "3");
}

#[test]
fn test_cast_double_keyword() {
    assert_eq!(run_php("<?php $x = (double)42; echo $x + 0.5;"), "42.5");
}

#[test]
fn test_cast_boolean_keyword() {
    assert_eq!(run_php("<?php echo (boolean)1 ? 'yes' : 'no';"), "yes");
}

// ========== Practical combined ==========

#[test]
fn test_practical_null_safe_default() {
    assert_eq!(
        run_php(
            "<?php
$config = null;
$value = isset($config) ? $config : 'default';
echo $value;
"
        ),
        "default"
    );
}

#[test]
fn test_practical_type_check_pattern() {
    assert_eq!(
        run_php(
            "<?php
$items = [1, 'two', 3, 'four', 5];
$sum = 0;
foreach ($items as $v) {
    if (is_int($v)) {
        $sum += $v;
    }
}
echo $sum;
"
        ),
        "9"
    );
}

#[test]
fn test_practical_isset_with_unset_loop() {
    assert_eq!(
        run_php(
            "<?php
$a = 1; $b = 2; $c = 3;
unset($b);
$result = '';
if (isset($a)) { $result .= 'a'; }
if (isset($b)) { $result .= 'b'; }
if (isset($c)) { $result .= 'c'; }
echo $result;
"
        ),
        "ac"
    );
}

#[test]
fn test_practical_cast_sum_strings() {
    assert_eq!(
        run_php(
            "<?php
$a = '10';
$b = '20';
echo (int)$a + (int)$b;
"
        ),
        "30"
    );
}

// ========== empty() with expressions ==========

#[test]
fn test_empty_expression_truthy() {
    assert_eq!(
        run_php("<?php $x = 1; echo empty($x + 1) ? 'yes' : 'no';"),
        "no"
    );
}

#[test]
fn test_empty_expression_falsy() {
    assert_eq!(
        run_php("<?php $x = 0; echo empty($x + 0) ? 'yes' : 'no';"),
        "yes"
    );
}

// ========== unset() on non-array ==========

#[test]
fn test_unset_dim_on_scalar_fatal() {
    let result = std::panic::catch_unwind(|| run_php("<?php $x = 42; unset($x[0]);"));
    assert!(result.is_err());
}

#[test]
fn test_unset_dim_on_null_silent() {
    assert_eq!(run_php("<?php $x = null; unset($x[0]); echo 'ok';"), "ok");
}

#[test]
fn test_unset_dim_on_undef_silent() {
    assert_eq!(run_php("<?php unset($x[0]); echo 'ok';"), "ok");
}
