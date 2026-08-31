mod common;

use common::{run_php, run_php_expect_error, run_php_with_source_context};

#[test]
fn object_defaults_materialize_at_each_php_ownership_boundary() {
    assert_eq!(
        run_php(
            r#"<?php
final class Box {
    public static int $next = 0;
    public int $id;
    public function __construct(public mixed $tag) {
        $this->id = ++self::$next;
        echo 'new:', $this->id, ':', is_object($tag) ? ('Box#' . $tag->id) : $tag, "\n";
    }
}
function collect($plain = new Box(__FUNCTION__), $nested = new Box(new Box('inner'))) {
    static $once = new Box(__FUNCTION__ . ':static');
    echo "use:$plain->id:$nested->id:$once->id\n";
    return [$plain, $nested, $once];
}
[$plain1, $nested1, $static1] = collect();
[$plain2, $nested2, $static2] = collect();
echo ($plain1 === $plain2 ? 'P=' : 'P!'),
     ($nested1 === $nested2 ? 'N=' : 'N!'),
     ($static1 === $static2 ? 'S=' : 'S!');
"#,
        ),
        concat!(
            "new:1:collect\n",
            "new:2:inner\n",
            "new:3:Box#2\n",
            "new:4:collect:static\n",
            "use:1:3:4\n",
            "new:5:collect\n",
            "new:6:inner\n",
            "new:7:Box#6\n",
            "use:5:7:4\n",
            "P!N!S=",
        )
    );
}

#[test]
fn failed_object_defaults_retry_without_entering_the_body_or_poisoning_static_state() {
    assert_eq!(
        run_php(
            r#"<?php
final class RetryToken {
    public static int $attempts = 0;
    public function __construct(bool $fail) {
        $attempt = ++self::$attempts;
        echo "create:$attempt\n";
        if ($fail) throw new RuntimeException("failure:$attempt");
    }
}
function defaultRetry($value = new RetryToken(true)) { echo "body:default\n"; }
for ($round = 0; $round < 2; ++$round) {
    try { defaultRetry(); } catch (RuntimeException $error) { echo $error->getMessage(), "\n"; }
}
defaultRetry(new RetryToken(false));

function staticRetry() {
    static $value = new RetryToken(true);
    echo "body:static\n";
}
for ($round = 0; $round < 2; ++$round) {
    try { staticRetry(); } catch (RuntimeException $error) { echo $error->getMessage(), "\n"; }
}
"#,
        ),
        concat!(
            "create:1\n",
            "failure:1\n",
            "create:2\n",
            "failure:2\n",
            "create:3\n",
            "body:default\n",
            "create:4\n",
            "failure:4\n",
            "create:5\n",
            "failure:5\n",
        )
    );
}

#[test]
fn constant_and_attribute_objects_keep_their_distinct_identity_rules() {
    assert_eq!(
        run_php(
            r#"<?php
final class Token {
    public static int $made = 0;
    public int $id;
    public function __construct(public string $tag) {
        $this->id = ++self::$made;
        echo "make:$this->id:$tag\n";
    }
}
#[Attribute]
class Holder { public function __construct(public object $value) {} }

echo "before-constant\n";
const GLOBAL_TOKEN = new Token('global');
echo "after-constant\n";
$first = GLOBAL_TOKEN;
$second = GLOBAL_TOKEN;
echo 'constant:', $first === $second ? 'same' : 'fresh', "\n";

#[Holder(new Token('attribute'))]
class Marked {}
echo "after-declaration\n";
$attribute = (new ReflectionClass(Marked::class))->getAttributes()[0];
$arg1 = $attribute->getArguments()[0];
$arg2 = $attribute->getArguments()[0];
echo 'arguments:', $arg1 === $arg2 ? 'same' : 'fresh', "\n";
$instance1 = $attribute->newInstance();
$instance2 = $attribute->newInstance();
echo 'instances:', $instance1->value === $instance2->value ? 'same' : 'fresh';
"#,
        ),
        concat!(
            "before-constant\n",
            "make:1:global\n",
            "after-constant\n",
            "constant:same\n",
            "after-declaration\n",
            "make:2:attribute\n",
            "make:3:attribute\n",
            "arguments:fresh\n",
            "make:4:attribute\n",
            "make:5:attribute\n",
            "instances:fresh",
        )
    );
}

#[test]
fn a_runtime_object_constant_keeps_one_identity_through_deferred_consumers() {
    assert_eq!(
        run_php(
            r#"<?php
const SHARED_TOKEN = new stdClass;
#[Attribute]
class SharedArgument { public function __construct(public object $value) {} }
#[SharedArgument(SHARED_TOKEN)]
class SharedConsumer {
    public object $instance = SHARED_TOKEN;
    public static object $static = SHARED_TOKEN;
    public const object CLASS_TOKEN = SHARED_TOKEN;
}
$consumer = new SharedConsumer;
$attribute = (new ReflectionClass(SharedConsumer::class))->getAttributes()[0];
$argument1 = $attribute->getArguments()[0];
$argument2 = $attribute->getArguments()[0];
var_dump(
    $consumer->instance === SHARED_TOKEN,
    SharedConsumer::$static === SHARED_TOKEN,
    SharedConsumer::CLASS_TOKEN === SHARED_TOKEN,
    $argument1 === SHARED_TOKEN,
    $argument1 === $argument2,
);
"#,
        ),
        concat!(
            "bool(true)\n",
            "bool(true)\n",
            "bool(true)\n",
            "bool(true)\n",
            "bool(true)\n",
        )
    );
}

#[test]
fn object_casts_follow_the_same_allowed_context_identity_boundaries() {
    assert_eq!(
        run_php(
            r#"<?php
function castDefault($value = (object) ['tag' => 'default']) {
    static $once = (object) ['tag' => 'static'];
    return [$value, $once];
}
[$default1, $static1] = castDefault();
[$default2, $static2] = castDefault();
echo 'default:', $default1 === $default2 ? 'same' : 'fresh',
     ':static:', $static1 === $static2 ? 'same' : 'fresh', "\n";

const CAST_CONSTANT = (object) ['tag' => 'constant'];
$constant1 = CAST_CONSTANT;
$constant2 = CAST_CONSTANT;
echo 'constant:', $constant1 === $constant2 ? 'same' : 'fresh', "\n";

#[Attribute]
class CastArgument { public function __construct(public object $value) {} }
#[CastArgument((object) ['tag' => 'attribute'])]
class CastTarget {}
$attribute = (new ReflectionClass(CastTarget::class))->getAttributes()[0];
$argument1 = $attribute->getArguments()[0];
$argument2 = $attribute->getArguments()[0];
echo 'attribute:', $argument1 === $argument2 ? 'same' : 'fresh', ':', $argument1->tag;
"#,
        ),
        "default:fresh:static:same\nconstant:same\nattribute:fresh:attribute"
    );
}

#[test]
fn null_coalescing_suppresses_only_missing_constant_expression_offsets() {
    assert_eq!(
        run_php(
            r#"<?php
const TREE = [7 => [['present' => null]]];
const FROM_MISSING_ROOT = TREE['missing']['deeper'] ?? 41;
const FROM_MISSING_BRANCH = TREE[7][0]['missing']['deeper'] ?? 42;
const FROM_NULL_LEAF = TREE[7][0]['present']['deeper'] ?? 43;
const FROM_PRESENT = TREE[7][0] ?? 44;
echo FROM_MISSING_ROOT, ':', FROM_MISSING_BRANCH, ':', FROM_NULL_LEAF, ':';
var_dump(FROM_PRESENT);
"#,
        ),
        "41:42:43:array(1) {\n  [\"present\"]=>\n  NULL\n}\n"
    );
}

#[test]
fn ordinary_deferred_reads_warn_and_continue_outside_a_coalescing_probe() {
    assert_eq!(
        run_php_with_source_context(
            r#"<?php
#[Attribute(+[["slot" => []]["slot"][9]?->missing]->tail)]
class DeferredWarningMarker {}
(new ReflectionClass(DeferredWarningMarker::class))->getAttributes()[0]->getArguments();
echo "continued";
"#,
            "<main>",
            "",
        ),
        concat!(
            "\nWarning: Undefined array key 9 in <main> on line 2\n",
            "\nWarning: Attempt to read property \"tail\" on array in <main> on line 2\n",
            "continued",
        )
    );
}

#[test]
fn failed_static_array_materialization_retries_without_committing_a_cell() {
    assert_eq!(
        run_php(
            r#"<?php
define('RUNTIME_VALUE', 31);
function invalidKey() {
    static $value = [RUNTIME_VALUE, [] => 'invalid'];
    echo "body\n";
}
for ($round = 1; $round <= 2; ++$round) {
    try { invalidKey(); }
    catch (TypeError $error) { echo "error:$round:", $error->getMessage(), "\n"; }
}
"#,
        ),
        concat!(
            "error:1:Cannot access offset of type array on array\n",
            "error:2:Cannot access offset of type array on array\n",
        )
    );
}

#[test]
fn unresolved_property_default_retries_after_the_symbol_becomes_available() {
    assert_eq!(
        run_php_with_source_context(
            r#"<?php
class DeferredSetting {
    public static $value = LATE_SETTING;
    public function __construct() { echo "construct\n"; }
}
for ($attempt = 1; $attempt <= 2; ++$attempt) {
    try {
        $object = new DeferredSetting();
        echo "ok:$attempt:", $object::$value, "\n";
    } catch (Error $error) {
        echo "error:$attempt:", $error->getMessage(), ':', $error->getLine(), "\n";
        define('LATE_SETTING', 73);
    }
}
"#,
            "/virtual/deferred-setting.php",
            "/virtual",
        ),
        "error:1:Undefined constant \"LATE_SETTING\":3\nconstruct\nok:2:73\n"
    );
}

#[test]
fn unresolved_class_constant_retries_without_publishing_a_failed_value() {
    assert_eq!(
        run_php(
            r#"<?php
class DeferredClassConstant { public const int VALUE = LATE_CLASS_VALUE; }
for ($attempt = 1; $attempt <= 2; ++$attempt) {
    try { echo DeferredClassConstant::VALUE, "\n"; }
    catch (Error $error) {
        echo "error:$attempt:", $error->getMessage(), "\n";
        if ($attempt === 1) define('LATE_CLASS_VALUE', 81);
    }
}
"#,
        ),
        "error:1:Undefined constant \"LATE_CLASS_VALUE\"\n81\n"
    );
}

#[test]
fn relative_new_defaults_bind_to_the_declaration_scope() {
    assert_eq!(
        run_php(
            r#"<?php
class Root {}
class ParentNode extends Root {
    public static function inherited($value = new parent) { echo 'inherited:', $value::class, "\n"; }
}
class ChildNode extends ParentNode {
    public static function own($self = new self, $parent = new parent) {
        echo 'own:', $self::class, ':', $parent::class, "\n";
    }
}
ChildNode::own();
ChildNode::inherited();
function invalidRelative($value = new self) {}
try { invalidRelative(); } catch (Error $error) { echo $error->getMessage(); }
"#,
        ),
        concat!(
            "own:ChildNode:ParentNode\n",
            "inherited:Root\n",
            "Cannot access \"self\" when no class scope is active",
        )
    );
}

#[test]
fn forbidden_constant_expression_forms_fail_at_declaration_validation() {
    for (source, expected) in [
        (
            "<?php class PropertyOwner { public $value = new stdClass; }",
            "New expressions are not supported in this context",
        ),
        (
            "<?php class ConstantOwner { public const VALUE = new stdClass; }",
            "New expressions are not supported in this context",
        ),
        (
            "<?php class CastOwner { public $value = (object) []; }",
            "Object casts are not supported in this context",
        ),
        (
            "<?php $owner = 'NamedOwner'; const VALUE = $owner::ITEM;",
            "Dynamic class names are not allowed in compile-time class constant references",
        ),
    ] {
        let error = run_php_expect_error(source);
        let expected = format!("{expected} on line 1");
        assert!(
            matches!(error, rphp::vm::execute::VmError::Fatal(ref message) if message == &expected),
            "expected exact compile diagnostic {expected:?}, got {error:?}",
        );
    }
}

#[test]
fn runtime_factories_do_not_postpone_invalid_constant_expression_forms() {
    for (source, expected) in [
        (
            "<?php function invalidCall($value = new stdClass(strlen('x'))) {}",
            "Constant expression contains invalid operations",
        ),
        (
            "<?php function dynamicClass($value = new $class) {}",
            "Cannot use dynamic class name in constant expression",
        ),
        (
            "<?php function anonymousClass($value = new class {}) {}",
            "Cannot use anonymous class in constant expression",
        ),
        (
            "<?php function unpackedArgument($value = new stdClass(...[])) {}",
            "Argument unpacking in constant expressions is not supported",
        ),
        (
            "<?php function castCall($value = (object) [strlen('x')]) {}",
            "Constant expression contains invalid operations",
        ),
    ] {
        let error = run_php_expect_error(source);
        let expected = format!("{expected} on line 1");
        assert!(
            matches!(error, rphp::vm::execute::VmError::Fatal(ref message) if message == &expected),
            "expected exact compile diagnostic {expected:?}, got {error:?}",
        );
    }
}

#[test]
fn static_variable_initializers_keep_their_broader_runtime_expression_surface() {
    assert_eq!(
        run_php(
            r#"<?php
const RUNTIME_CLASS = 'stdClass';
$source = 17;
function materialize() { return 19; }
static $dynamic = new (RUNTIME_CLASS);
static $anonymous = new class { public int $value = 2; };
static $unpacked = new stdClass(...[]);
static $variableArgument = new stdClass($source);
static $called = materialize();
echo $dynamic::class, ':', $anonymous->value, ':', $unpacked::class, ':',
     $variableArgument::class, ':', $called;
"#,
        ),
        "stdClass:2:stdClass:stdClass:19"
    );
}
