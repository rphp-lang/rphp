mod common;

use common::{run_php, run_php_with_source_context};

#[test]
fn callable_parameters_resolve_static_and_instance_method_shapes() {
    assert_eq!(
        run_php(
            r#"<?php
class CallableShape {
    public function instance(): void {}
    public static function staticMethod(): void {}
}
function accept(callable $callback): void { echo "accepted\n"; }
foreach ([[CallableShape::class, 'instance'], [CallableShape::class, 'staticMethod']] as $callback) {
    try { accept($callback); }
    catch (TypeError $error) { echo $error->getMessage(), "\n"; }
}
"#,
        ),
        concat!(
            "accept(): Argument #1 ($callback) must be of type callable, array given, called in <main> on line 8\n",
            "accepted\n",
        )
    );
}

#[test]
fn relative_type_diagnostics_publish_resolved_public_class_names() {
    let output = run_php(
        r#"<?php
namespace TypeBoundary;
class ParentType {}
class ChildType extends ParentType {
    public function selfParam(self $value): void {}
    public function parentParam(parent $value): void {}
}
$value = new ChildType;
foreach (['selfParam', 'parentParam'] as $method) {
    try { $value->$method(1); }
    catch (\TypeError $error) { echo $error->getMessage(), "\n"; }
}
$anonymous = new class { public function test(self|string $value): self|string { return $value; } };
try { $anonymous->test(null); }
catch (\TypeError $error) { echo $error->getMessage(), "\n"; }
"#,
    );
    assert!(
        output.contains("TypeBoundary\\ChildType::selfParam(): Argument #1 ($value) must be of type TypeBoundary\\ChildType, int given"),
        "{output}"
    );
    assert!(
        output.contains("TypeBoundary\\ChildType::parentParam(): Argument #1 ($value) must be of type TypeBoundary\\ParentType, int given"),
        "{output}"
    );
    assert!(
        output.contains("class@anonymous(): Argument #1 ($value)"),
        "{output}"
    );
    assert!(
        output.contains("class@anonymous|string, null given"),
        "{output}"
    );
}

#[test]
fn too_few_user_arguments_originate_at_the_declaration_and_keep_the_pending_call() {
    let output = run_php_with_source_context(
        "<?php\nfunction needsOne($value) {}\ntry { needsOne(); } catch (Throwable $error) { echo $error->getFile(), ':', $error->getLine(), '|', $error->getTrace()[0]['function']; }\n",
        "/virtual/arity-origin.php",
        "/virtual",
    );
    assert_eq!(output, "/virtual/arity-origin.php:2|needsOne");
}

#[test]
fn returning_a_reference_from_a_void_callable_is_deprecated_at_declaration_time() {
    let output = run_php_with_source_context(
        "<?php\nfunction &voidReference(): void { return; }\nvoidReference();\n",
        "/virtual/void-reference.php",
        "/virtual",
    );
    assert!(
        output
            .contains("voidReference(): Returning by reference from a void function is deprecated"),
        "{output}"
    );
}

#[test]
fn array_iterator_object_references_enforce_typed_and_readonly_properties() {
    assert_eq!(
        run_php(
            r#"<?php
error_reporting(E_ALL & ~E_DEPRECATED);
class TypedStorage { public string $value = 'initial'; }
$typed = new TypedStorage;
$iterator = new ArrayIterator($typed);
foreach ($iterator as &$value) {
    try { $value = []; }
    catch (TypeError $error) { echo $error->getMessage(), "\n"; }
}
var_dump($typed->value);

class ReadonlyStorage { public function __construct(public readonly string $value) {} }
$readonly = new ReadonlyStorage('locked');
try { foreach (new ArrayIterator($readonly) as &$value) {} }
catch (Error $error) { echo $error->getMessage(), "\n"; }
var_dump($readonly->value);
"#,
        ),
        concat!(
            "Cannot assign array to reference held by property TypedStorage::$value of type string\n",
            "string(7) \"initial\"\n",
            "Cannot acquire reference to readonly property ReadonlyStorage::$value\n",
            "string(6) \"locked\"\n",
        )
    );
}

#[test]
fn main_scope_reference_arguments_remain_shared_with_globals_writes() {
    assert_eq!(
        run_php(
            r#"<?php
class TypedObject { public int $value = 42; }
function replaceThroughGlobals(TypedObject &$value): void {
    $GLOBALS['shared'] = new stdClass;
    $GLOBALS['shared']->value = 3.5;
    var_dump(is_float($value->value));
}
$shared = new TypedObject;
replaceThroughGlobals($shared);
var_dump(is_float($shared->value));
"#,
        ),
        "bool(true)\nbool(true)\n"
    );
}

#[test]
fn value_only_incdec_preserves_typed_property_reference_constraints() {
    assert_eq!(
        run_php(
            r#"<?php
$value = new class implements ArrayAccess {
    public int $number = PHP_INT_MAX;
    public function offsetExists($offset): bool { return true; }
    public function &offsetGet($offset): mixed { return $this->number; }
    public function offsetSet($offset, $value): void {}
    public function offsetUnset($offset): void {}
};
try { $value[0]++; }
catch (Error $error) { echo $error->getMessage(), "\n"; }
var_dump($value->number);
"#,
        ),
        concat!(
            "Cannot increment a reference held by property ArrayAccess@anonymous::$number of type int past its maximal value\n",
            "int(9223372036854775807)\n",
        )
    );
}
