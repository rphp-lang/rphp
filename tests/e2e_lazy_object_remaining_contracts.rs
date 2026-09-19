mod common;
use common::run_php;

#[test]
fn lazy_creation_rejects_internal_classes_and_descendants_with_errors() {
    assert_eq!(
        run_php(
            r#"<?php
class DateChild extends DateTime {}
foreach ([DateTime::class, DateChild::class] as $class) {
    $reflection = new ReflectionClass($class);
    foreach (['newLazyGhost', 'newLazyProxy'] as $method) {
        try {
            $reflection->$method(fn () => new $class());
        } catch (Error $error) {
            echo $error->getMessage(), "\n";
        }
    }
}
"#,
        ),
        concat!(
            "Cannot make instance of internal class lazy: DateTime is internal\n",
            "Cannot make instance of internal class lazy: DateTime is internal\n",
            "Cannot make instance of internal class lazy: DateChild inherits internal class DateTime\n",
            "Cannot make instance of internal class lazy: DateChild inherits internal class DateTime\n",
        )
    );
}

#[test]
fn reference_assignment_binds_before_following_binary_operators() {
    assert_eq!(
        run_php(
            r#"<?php
$source = 5;
$other = 2;
var_dump($alias =& $source - $other);
$alias = 9;
var_dump($source);

class Box { public int $value = 7; public mixed $alias; }
$box = new Box;
var_dump($box->alias =& $box->value - 3 > $tail = 1);
$box->alias = 11;
var_dump($box->value, $tail);
"#,
        ),
        concat!(
            "int(3)\n",
            "int(9)\n",
            "bool(true)\n",
            "int(11)\n",
            "int(1)\n",
        )
    );
}

#[test]
fn lazy_proxy_factory_validates_compatible_storage_and_magic_lifecycle() {
    assert_eq!(
        run_php(
            r#"<?php
class Base { public int $value = 1; }
class Compatible extends Base {}
class Additional extends Base { public int $extra = 2; }
class DestructorOverride extends Base { public function __destruct() {} }

function attempt(string $proxy, mixed $result): void {
    $lazy = (new ReflectionClass($proxy))->newLazyProxy(fn () => $result);
    try {
        (new ReflectionClass($proxy))->initializeLazyObject($lazy);
        echo "accepted:", $lazy->value, "\n";
    } catch (Throwable $error) {
        echo get_class($error), ':', $error->getMessage(), "\n";
    }
}

attempt(Compatible::class, new Base());
attempt(Additional::class, new Base());
attempt(DestructorOverride::class, new Base());
attempt(Base::class, null);
$reflection = new ReflectionClass(Base::class);
$lazy = $reflection->newLazyProxy(fn ($object) => $object);
try { $reflection->initializeLazyObject($lazy); } catch (Throwable $error) {
    echo get_class($error), ':', $error->getMessage(), "\n";
}
"#,
        ),
        concat!(
            "accepted:1\n",
            "TypeError:The real instance class Base is not compatible with the proxy class Additional. The proxy must be a instance of the same class as the real instance, or a sub-class with no additional properties, and no overrides of the __destructor or __clone methods.\n",
            "TypeError:The real instance class Base is not compatible with the proxy class DestructorOverride. The proxy must be a instance of the same class as the real instance, or a sub-class with no additional properties, and no overrides of the __destructor or __clone methods.\n",
            "TypeError:Lazy proxy factory must return an instance of a class compatible with Base, null returned\n",
            "Error:Lazy proxy factory must return a non-lazy object\n",
        )
    );
}

#[test]
fn recursive_magic_reference_materializes_dynamic_property_on_object_and_proxy() {
    assert_eq!(
        run_php(
            r#"<?php
error_reporting(E_ALL);
set_error_handler(function (int $severity, string $message): bool {
    $kind = match ($severity) {
        E_DEPRECATED => 'Deprecated',
        E_NOTICE => 'Notice',
        default => 'Diagnostic',
    };
    echo $kind, ': ', $message, "\n";
    return true;
});
class C {
    public $backing;
    public function &__get($name) { return $this->x; }
}

$ordinary = new C;
$ordinary->x;

$reflection = new ReflectionClass(C::class);
$proxy = $reflection->newLazyProxy(fn () => new C);
$proxy->x;

class A {
    public $_;
    public function __get($name) {
        global $lazy;
        $lazy->x =& $this->_;
    }
}
$reflection = new ReflectionClass(A::class);
$lazy = $reflection->newLazyProxy(fn () => new A);
$reflection->initializeLazyObject($lazy);
try { $lazy->p; } catch (Throwable $error) {
    echo get_class($error), ': ', $error->getMessage(), "\n";
}
"#,
        ),
        concat!(
            "Deprecated: Creation of dynamic property C::$x is deprecated\n",
            "Deprecated: Creation of dynamic property C::$x is deprecated\n",
            "Deprecated: Creation of dynamic property A::$x is deprecated\n",
            "Notice: Indirect modification of overloaded property A::$x has no effect\n",
            "Error: Cannot assign by reference to overloaded object\n",
        )
    );
}

#[test]
fn released_lazy_object_raises_error_and_preserves_initializer_failure() {
    assert_eq!(
        run_php(
            r#"<?php
class C { public $value; }

function show(Throwable $error): void {
    do { echo get_class($error), ':', $error->getMessage(), "\n"; }
    while ($error = $error->getPrevious());
}

foreach (['newLazyGhost', 'newLazyProxy'] as $method) {
    $reflection = new ReflectionClass(C::class);
    $object = $reflection->$method(function ($shell) use ($method) {
        global $object;
        $object = null;
        return $method === 'newLazyProxy' ? new C : null;
    });
    try { $object->value = 1; } catch (Throwable $error) { show($error); }
}

$reflection = new ReflectionClass(C::class);
$object = $reflection->newLazyGhost(function ($shell) {
    global $object;
    $object = null;
    throw new Exception('initializer');
});
try { $object->value; } catch (Throwable $error) { show($error); }

$object = $reflection->newLazyGhost(function ($shell) {
    global $object;
    $object->value = $object;
    $object = null;
});
try { $object->value = 1; echo "cycle-ok\n"; } catch (Throwable $error) { show($error); }
"#,
        ),
        concat!(
            "Error:Lazy object was released during initialization\n",
            "Error:Lazy object was released during initialization\n",
            "Error:Lazy object was released during initialization\n",
            "Exception:initializer\n",
            "cycle-ok\n",
        )
    );
}

#[test]
fn nested_function_is_published_at_runtime_and_redeclares_through_clone_callback() {
    assert_eq!(
        run_php(
            r#"<?php
function installNested() { function nestedRuntimeFunction() {} }
var_dump(function_exists('nestedRuntimeFunction'));
installNested();
var_dump(function_exists('nestedRuntimeFunction'));
"#,
        ),
        "bool(false)\nbool(true)\n"
    );

    assert_eq!(
        common::run_php_expect_error_with_source_context(
            r#"<?php
function installFromClone() { function cloneRuntimeFunction() {} }
class CloneTrigger { public function __clone() { installFromClone(); } }
installFromClone();
clone new CloneTrigger;
"#,
            "/virtual/lazy-runtime-function.php",
            "/virtual",
        )
        .to_string(),
        concat!(
            "Cannot redeclare function cloneRuntimeFunction() ",
            "(previously declared in /virtual/lazy-runtime-function.php:2) ",
            "in /virtual/lazy-runtime-function.php on line 2",
        )
    );
}

#[test]
fn lazy_ghost_clone_materializes_deferred_property_defaults_before_initializer() {
    assert_eq!(
        run_php(
            r#"<?php
class MissingDefault { public $value = MISSING_LAZY_DEFAULT; }
$reflection = new ReflectionClass(MissingDefault::class);
$lazy = $reflection->newLazyGhost(function () { echo "initializer-must-not-run\n"; });
try { clone $lazy; } catch (Throwable $error) {
    echo get_class($error), ':', $error->getMessage(), "\n";
}

class LateDefault { public $value = LATE_LAZY_DEFAULT; }
$reflection = new ReflectionClass(LateDefault::class);
$lazy = $reflection->newLazyGhost(function () { echo "initialized\n"; });
define('LATE_LAZY_DEFAULT', 42);
$clone = clone $lazy;
var_dump($clone->value);
"#,
        ),
        concat!(
            "Error:Undefined constant \"MISSING_LAZY_DEFAULT\"\n",
            "initialized\n",
            "int(42)\n",
        )
    );
}
