mod common;
use common::*;

// ============================================================================
// class-like constants
// ============================================================================

#[test]
fn test_class_constants_support_forward_references_types_and_class_name() {
    let out = run_php(
        r#"<?php
class Values {
    public const FIRST = self::SECOND;
    public const SECOND = 42, TEXT = "ok";
    public const COMPUTED = self::FIRST + 1;
    public const MESSAGE = self::TEXT . "!";
    final protected const string LABEL = "typed";

    public static function label() {
        return self::LABEL;
    }
}
echo Values::FIRST . ':' . Values::SECOND . ':' . Values::COMPUTED . ':';
echo Values::MESSAGE . ':';
echo Values::label() . ':' . Values::class . ':' . MissingClass::class;
"#,
    );
    assert_eq!(out, "42:42:43:ok!:typed:Values:MissingClass");
}

#[test]
fn test_class_constants_are_composed_and_late_static_reads_are_cached_by_class() {
    let out = run_php(
        r#"<?php
interface Numbered { public const INTERFACE_VALUE = 7; }
trait Tagged {
    protected const TRAIT_VALUE = 8;
    private const PRIVATE_VALUE = 9;
    public static function privateValue() { return self::PRIVATE_VALUE; }
}
class ConstantBase {
    protected const VALUE = 10;
    public static function value() { return static::VALUE; }
}
class ConstantChild extends ConstantBase implements Numbered {
    use Tagged;
    public const VALUE = 11;
    public static function combined() {
        return self::INTERFACE_VALUE + self::TRAIT_VALUE;
    }
}
echo ConstantBase::value() . ':' . ConstantChild::value() . ':';
echo ConstantChild::value() . ':' . ConstantChild::combined() . ':';
echo ConstantChild::privateValue() . ':' . ConstantChild::INTERFACE_VALUE;
"#,
    );
    assert_eq!(out, "10:11:11:15:9:7");
}

#[test]
fn test_class_constant_visibility_errors_are_catchable() {
    let out = run_php(
        r#"<?php
class SecretConstants {
    private const PRIVATE_VALUE = 1;
    protected const PROTECTED_VALUE = 2;
}
try { echo SecretConstants::PRIVATE_VALUE; } catch (Error $error) { echo "private"; }
echo ':';
try { echo SecretConstants::PROTECTED_VALUE; } catch (Error $error) { echo "protected"; }
"#,
    );
    assert_eq!(out, "private:protected");
}

#[test]
fn test_final_and_typed_class_constant_contracts_are_validated() {
    let final_error = run_php_expect_error(
        r#"<?php
class FinalConstantBase { final public const VALUE = 1; }
class FinalConstantChild extends FinalConstantBase { public const VALUE = 2; }
"#,
    );
    assert!(format!("{final_error:?}").contains("cannot override final constant"));

    let type_error = run_php_expect_error(
        r#"<?php
class TypedConstant { public const int VALUE = "wrong"; }
"#,
    );
    assert!(format!("{type_error:?}").contains("for class constant TypedConstant::VALUE"));
}

#[test]
fn test_dynamic_class_constant_owners_and_names_rekey_one_cache_site() {
    let out = run_php(
        r#"<?php
class DynamicA {
    public const FIRST = "a1";
    public const SECOND = "a2";
}
class DynamicB {
    public const FIRST = "b1";
    public const SECOND = "b2";
}
function fixedConstant($owner) { return $owner::FIRST; }
function namedConstant($owner, $name) { return $owner::{$name}; }

echo fixedConstant(DynamicA::class) . ':';
echo fixedConstant(DynamicB::class) . ':';
echo fixedConstant(new DynamicA()) . ':';
echo fixedConstant(new DynamicB()) . ':';
echo namedConstant(DynamicA::class, 'FIRST') . ':';
echo namedConstant(DynamicA::class, 'SECOND') . ':';
echo namedConstant(DynamicB::class, 'FIRST') . ':';
echo namedConstant(new DynamicB(), 'SECOND');
"#,
    );
    assert_eq!(out, "a1:b1:a1:b1:a1:a2:b1:b2");
}

#[test]
fn test_dynamic_class_constants_preserve_evaluation_late_static_and_visibility() {
    let out = run_php(
        r#"<?php
class DynamicBase {
    protected const SECRET = "base-secret";
    public const VALUE = "base";
    public static function late($name) { return static::{$name}; }
    public static function lexical($name) { return self::{$name}; }
    public function own($name) { return $this::{$name}; }
}
class DynamicChild extends DynamicBase {
    public const VALUE = "child";
}
function ownerExpression() { echo 'O'; return DynamicChild::class; }
function nameExpression() { echo 'N'; return 'VALUE'; }

echo DynamicBase::late('VALUE') . ':' . DynamicChild::late('VALUE') . ':';
echo DynamicChild::lexical('VALUE') . ':';
echo (new DynamicBase())->own('SECRET') . ':';
echo ownerExpression()::{nameExpression()};
try {
    $name = 'SECRET';
    echo DynamicBase::{$name};
} catch (Error $error) {
    echo ':' . $error->getMessage();
}
"#,
    );
    assert_eq!(
        out,
        "base:child:base:base-secret:ONchild:Cannot access protected constant DynamicBase::SECRET"
    );
}

#[test]
fn test_dynamic_class_keyword_distinguishes_runtime_and_compile_time_names() {
    let out = run_php(
        r#"<?php
class DynamicClassName { public const CLASS_NAME = 'class'; }
const DYNAMIC_CLASS_KEYWORD = 'class';
$object = new DynamicClassName();
$owner = DynamicClassName::class;
$name = 'class';

echo $object::class . ':';
echo $owner::{$name} . ':';
echo DynamicClassName::{$name} . ':';
echo DynamicClassName::{DYNAMIC_CLASS_KEYWORD} . ':';
try { echo $owner::class; } catch (TypeError $error) { echo $error->getMessage(); }
echo ':';
try { echo DynamicClassName::{"class"}; } catch (Error $error) { echo $error->getMessage(); }
echo ':';
try { echo $owner::{"cl" . "ass"}; } catch (Error $error) { echo $error->getMessage(); }
echo ':';
try { echo DynamicClassName::{DynamicClassName::CLASS_NAME}; }
catch (Error $error) { echo $error->getMessage(); }
echo ':';
try { echo DynamicClassName::{true ? 'class' : 'missing'}; }
catch (Error $error) { echo $error->getMessage(); }
"#,
    );
    assert_eq!(
        out,
        "DynamicClassName:DynamicClassName:DynamicClassName:DynamicClassName:Cannot use \"::class\" on string:Undefined constant DynamicClassName::class:Undefined constant DynamicClassName::class:Undefined constant DynamicClassName::class:Undefined constant DynamicClassName::class"
    );
}

#[test]
fn test_dynamic_class_constant_type_errors_follow_php_resolution_order() {
    let out = run_php(
        r#"<?php
class DynamicTypeOwner { public const VALUE = 1; }
function dynamicTypeFetch($owner, $name) { return $owner::{$name}; }

try { dynamicTypeFetch(42, 42); }
catch (Error $error) { echo $error->getMessage(); }
echo ':';
try { dynamicTypeFetch('MissingDynamicOwner', 42); }
catch (Error $error) { echo $error->getMessage(); }
echo ':';
try { dynamicTypeFetch(DynamicTypeOwner::class, 42); }
catch (TypeError $error) { echo $error->getMessage(); }
"#,
    );
    assert_eq!(
        out,
        "Class name must be a valid object or a string:Class \"MissingDynamicOwner\" not found:Cannot use value of type int as class constant name"
    );
}

#[test]
fn test_dynamic_enum_cases_and_constants_do_not_alias_cache_entries() {
    let out = run_php(
        r#"<?php
enum DynamicSuit {
    case Hearts;
    case Spades;
    public const LABEL = 'suit';
}
function dynamicEnumMember($name) { return DynamicSuit::{$name}; }

echo dynamicEnumMember('LABEL') . ':';
echo (dynamicEnumMember('Hearts') === DynamicSuit::Hearts ? 'heart' : 'bad') . ':';
echo dynamicEnumMember('LABEL') . ':';
echo (dynamicEnumMember('Spades') === DynamicSuit::Spades ? 'spade' : 'bad');
"#,
    );
    assert_eq!(out, "suit:heart:suit:spade");
}

#[test]
fn test_dynamic_class_constant_arrow_capture_and_generator_suspension() {
    let out = run_php(
        r#"<?php
class DynamicSuspended { public const VALUE = 'resolved'; }
$owner = DynamicSuspended::class;
$name = 'VALUE';
$fetch = fn() => $owner::{$name};
echo $fetch() . ':';

function suspendedDynamicConstant() {
    return (yield 'owner')::{yield 'name'};
}
$generator = suspendedDynamicConstant();
echo $generator->current() . ':';
echo $generator->send(DynamicSuspended::class) . ':';
$generator->send('VALUE');
echo $generator->getReturn();
"#,
    );
    assert_eq!(out, "resolved:owner:name:resolved");
}

#[test]
fn test_dynamic_class_constant_fetches_are_valid_constant_expressions() {
    let out = run_php(
        r#"<?php
class DynamicConstantExpression {
    public const BA = 'BA';
    public const R = 'R';
    public const BAR = 'bar';
    public const DynamicConstantExpression = 'bar';
    public const FIRST = self::{'BAR'};
    public const SECOND = self::{'BA' . 'R'};
    public const THIRD = self::{self::BA . self::R};
}
const DYNAMIC_CONST_EXPRESSION = DynamicConstantExpression::{DynamicConstantExpression::class};
echo DynamicConstantExpression::FIRST . ':';
echo DynamicConstantExpression::SECOND . ':';
echo DynamicConstantExpression::THIRD . ':';
echo DYNAMIC_CONST_EXPRESSION;
"#,
    );
    assert_eq!(out, "bar:bar:bar:bar");
}

// ============================================================================
// const keyword
// ============================================================================

#[test]
fn test_const_basic() {
    let out = run_php(
        r#"<?php
const FOO = 42;
echo FOO;
"#,
    );
    assert_eq!(out, "42");
}

#[test]
fn test_const_string() {
    let out = run_php(
        r#"<?php
const GREETING = "hello";
echo GREETING;
"#,
    );
    assert_eq!(out, "hello");
}

#[test]
fn test_const_in_expression() {
    let out = run_php(
        r#"<?php
const X = 10;
const Y = 20;
echo X + Y;
"#,
    );
    assert_eq!(out, "30");
}

#[test]
fn test_const_bool_and_null() {
    let out = run_php(
        r#"<?php
const A = true;
const B = false;
const C = null;
echo A;
echo B;
echo C;
"#,
    );
    assert_eq!(out, "1");
}

#[test]
fn test_const_float() {
    let out = run_php(
        r#"<?php
const PI = 3.14;
echo PI;
"#,
    );
    assert_eq!(out, "3.14");
}

#[test]
fn test_const_used_in_function() {
    let out = run_php(
        r#"<?php
const MAX = 100;
function getMax() {
    return MAX;
}
echo getMax();
"#,
    );
    assert_eq!(out, "100");
}

#[test]
fn test_const_in_condition() {
    let out = run_php(
        r#"<?php
const DEBUG = true;
if (DEBUG) {
    echo "debug on";
} else {
    echo "debug off";
}
"#,
    );
    assert_eq!(out, "debug on");
}

// ============================================================================
// define() function
// ============================================================================

#[test]
fn test_define_basic() {
    let out = run_php(
        r#"<?php
define("BAR", 99);
echo BAR;
"#,
    );
    assert_eq!(out, "99");
}

#[test]
fn test_define_string_value() {
    let out = run_php(
        r#"<?php
define("APP_NAME", "MyApp");
echo APP_NAME;
"#,
    );
    assert_eq!(out, "MyApp");
}

#[test]
fn test_defined_true() {
    let out = run_php(
        r#"<?php
const THING = 1;
if (defined("THING")) {
    echo "yes";
} else {
    echo "no";
}
"#,
    );
    assert_eq!(out, "yes");
}

#[test]
fn test_defined_false() {
    let out = run_php(
        r#"<?php
if (defined("NOPE")) {
    echo "yes";
} else {
    echo "no";
}
"#,
    );
    assert_eq!(out, "no");
}

#[test]
fn test_constant_function() {
    let out = run_php(
        r#"<?php
define("KEY", "value123");
echo constant("KEY");
"#,
    );
    assert_eq!(out, "value123");
}

#[test]
fn test_define_and_const_coexist() {
    let out = run_php(
        r#"<?php
const A = 1;
define("B", 2);
echo A + B;
"#,
    );
    assert_eq!(out, "3");
}

// ============================================================================
// Default parameter values
// ============================================================================

#[test]
fn test_default_param_basic() {
    let out = run_php(
        r#"<?php
function greet($name = "World") {
    echo "Hello " . $name;
}
greet();
"#,
    );
    assert_eq!(out, "Hello World");
}

#[test]
fn test_default_param_override() {
    let out = run_php(
        r#"<?php
function greet($name = "World") {
    echo "Hello " . $name;
}
greet("PHP");
"#,
    );
    assert_eq!(out, "Hello PHP");
}

#[test]
fn test_default_param_multiple() {
    let out = run_php(
        r#"<?php
function add($a, $b = 10, $c = 20) {
    return $a + $b + $c;
}
echo add(1);
echo " ";
echo add(1, 2);
echo " ";
echo add(1, 2, 3);
"#,
    );
    assert_eq!(out, "31 23 6");
}

#[test]
fn test_default_param_null() {
    let out = run_php(
        r#"<?php
function test($x = null) {
    if ($x === null) {
        echo "null";
    } else {
        echo $x;
    }
}
test();
"#,
    );
    assert_eq!(out, "null");
}

#[test]
fn test_default_param_bool() {
    let out = run_php(
        r#"<?php
function check($verbose = false) {
    if ($verbose) {
        echo "verbose";
    } else {
        echo "quiet";
    }
}
check();
echo " ";
check(true);
"#,
    );
    assert_eq!(out, "quiet verbose");
}

#[test]
fn test_default_param_integer() {
    let out = run_php(
        r#"<?php
function repeat($str, $times = 3) {
    $result = "";
    for ($i = 0; $i < $times; $i++) {
        $result .= $str;
    }
    echo $result;
}
repeat("a");
echo " ";
repeat("b", 2);
"#,
    );
    assert_eq!(out, "aaa bb");
}

#[test]
fn test_default_param_in_class_method() {
    let out = run_php(
        r#"<?php
class Greeter {
    public function hello($name = "World") {
        echo "Hi " . $name;
    }
}
$g = new Greeter();
$g->hello();
echo " ";
$g->hello("PHP");
"#,
    );
    assert_eq!(out, "Hi World Hi PHP");
}

#[test]
fn test_default_param_in_closure() {
    let out = run_php(
        r#"<?php
$add = function($a, $b = 5) {
    return $a + $b;
};
echo $add(10);
echo " ";
echo $add(10, 20);
"#,
    );
    assert_eq!(out, "15 30");
}

#[test]
fn test_default_param_expression() {
    let out = run_php(
        r#"<?php
function test($x = 2 + 3) {
    echo $x;
}
test();
"#,
    );
    assert_eq!(out, "5");
}

#[test]
fn test_default_param_string_concat() {
    let out = run_php(
        r#"<?php
function test($prefix = "Hello" . " " . "World") {
    echo $prefix;
}
test();
"#,
    );
    assert_eq!(out, "Hello World");
}

// ============================================================================
// Edge cases
// ============================================================================

#[test]
fn test_const_with_function_call() {
    let out = run_php(
        r#"<?php
const SEPARATOR = "-";
function join_parts($a, $b) {
    return $a . SEPARATOR . $b;
}
echo join_parts("hello", "world");
"#,
    );
    assert_eq!(out, "hello-world");
}

#[test]
fn test_many_defaults() {
    let out = run_php(
        r#"<?php
function config($host = "localhost", $port = 3306, $db = "test") {
    echo $host . ":" . $port . "/" . $db;
}
config();
echo "|";
config("prod");
echo "|";
config("prod", 5432);
echo "|";
config("prod", 5432, "mydb");
"#,
    );
    assert_eq!(
        out,
        "localhost:3306/test|prod:3306/test|prod:5432/test|prod:5432/mydb"
    );
}

#[test]
fn test_define_with_variable() {
    let out = run_php(
        r#"<?php
$val = 42;
define("DYNAMIC", $val);
echo DYNAMIC;
"#,
    );
    assert_eq!(out, "42");
}

// ============================================================================
// Regression: P1 — side-effect defaults must NOT run when arg is passed
// ============================================================================

#[test]
fn test_default_side_effect_skipped_when_arg_passed() {
    // The default calls a function with a side effect (echo).
    // When arg IS passed, the default must NOT be evaluated.
    let out = run_php(
        r#"<?php
function side() {
    echo "SIDE";
    return 99;
}
function test($x = side()) {
    echo $x;
}
test(5);
"#,
    );
    // Must output "5" only — "SIDE" must NOT appear
    assert_eq!(out, "5");
}

#[test]
fn test_default_side_effect_runs_when_arg_omitted() {
    // When arg is NOT passed, the default expression IS evaluated
    let out = run_php(
        r#"<?php
function side() {
    echo "SIDE";
    return 99;
}
function test($x = side()) {
    echo $x;
}
test();
"#,
    );
    assert_eq!(out, "SIDE99");
}

#[test]
fn test_default_side_effect_mixed() {
    // Multiple calls: first with arg (no side effect), second without (side effect)
    let out = run_php(
        r#"<?php
$count = 0;
function counter() {
    echo "C";
    return 1;
}
function test($a, $b = counter()) {
    echo $a . $b;
}
test(1, 2);
echo "|";
test(3);
"#,
    );
    assert_eq!(out, "12|C31");
}

// ============================================================================
// Regression: P2 — define() with non-string name uses string coercion
// ============================================================================

#[test]
fn test_define_integer_name_coerces_to_string() {
    // PHP coerces integer name to string "123"
    let out = run_php(
        r#"<?php
define("123", "val");
echo defined("123") ? "yes" : "no";
echo " ";
echo constant("123");
"#,
    );
    assert_eq!(out, "yes val");
}

#[test]
fn test_define_returns_false_for_empty_name() {
    let out = run_php(
        r#"<?php
$result = define("", "val");
echo $result ? "true" : "false";
"#,
    );
    assert_eq!(out, "false");
}

// ============================================================================
// Class property defaults — eval_const_expr coverage
// ============================================================================

#[test]
fn test_class_property_default_array_indexed() {
    let out = run_php(
        r#"<?php
class Config {
    public $items = [1, 2, 3];
}
$c = new Config();
echo count($c->items);
echo " ";
echo $c->items[1];
"#,
    );
    assert_eq!(out, "3 2");
}

#[test]
fn test_class_property_default_array_keyed() {
    let out = run_php(
        r#"<?php
class Config {
    public $opts = ["host" => "localhost", "port" => 3306];
}
$c = new Config();
echo $c->opts["host"];
echo ":";
echo $c->opts["port"];
"#,
    );
    assert_eq!(out, "localhost:3306");
}

#[test]
fn test_class_property_default_nested_array() {
    let out = run_php(
        r#"<?php
class C {
    public $data = [1, [2, 3]];
}
$c = new C();
echo count($c->data);
echo " ";
echo $c->data[0];
"#,
    );
    assert_eq!(out, "2 1");
}

#[test]
fn test_class_property_default_empty_array() {
    let out = run_php(
        r#"<?php
class C {
    public $items = [];
}
$c = new C();
echo count($c->items);
"#,
    );
    assert_eq!(out, "0");
}

#[test]
fn test_class_property_default_negative_int() {
    let out = run_php(
        r#"<?php
class C {
    public $x = -42;
}
$c = new C();
echo $c->x;
"#,
    );
    assert_eq!(out, "-42");
}

#[test]
fn test_class_property_default_negative_float() {
    let out = run_php(
        r#"<?php
class C {
    public $x = -3.14;
}
$c = new C();
echo $c->x;
"#,
    );
    assert_eq!(out, "-3.14");
}

#[test]
fn test_class_property_default_all_scalar_types() {
    let out = run_php(
        r#"<?php
class C {
    public $a = 42;
    public $b = 3.14;
    public $c = "hello";
    public $d = true;
    public $e = false;
    public $f = null;
}
$c = new C();
echo $c->a . " " . $c->b . " " . $c->c . " " . $c->d;
"#,
    );
    assert_eq!(out, "42 3.14 hello 1");
}

#[test]
fn test_class_property_default_function_call_is_compile_error() {
    let tokens = rphp::lexer::Lexer::new(
        r#"<?php
class C { public $x = strlen("hi"); }
"#,
    )
    .tokenize()
    .unwrap();
    let stmts = rphp::parser::Parser::new(tokens).parse().unwrap();
    let result = rphp::compiler::compile::Compiler::new().compile(&stmts);
    assert!(
        result.is_err(),
        "Function call in property default should be a compile error"
    );
}

// ============================================================================
// Constant in class property default resolves correctly
// ============================================================================

#[test]
fn test_const_in_class_property_default_resolves() {
    // User-defined constants from the same file are available in property defaults.
    let result = run_php(
        r#"<?php
const FOO = 42;
class C { public $x = FOO; }
$c = new C();
echo $c->x;
"#,
    );
    assert_eq!(result, "42");
}

// ============================================================================
// Regression: P2 — constant as function default (runtime eval — works fine)
// ============================================================================

#[test]
fn test_const_used_as_function_default() {
    // Constants work fine as function default expressions (evaluated at runtime)
    let out = run_php(
        r#"<?php
const DEFAULT_PORT = 8080;
function connect($port = DEFAULT_PORT) {
    echo $port;
}
connect();
echo " ";
connect(3000);
"#,
    );
    assert_eq!(out, "8080 3000");
}

// ── Namespace constant resolution ────────────────────────────────

#[test]
fn test_const_in_namespace_prescan() {
    // Constants defined inside a namespace block should be pre-scanned
    // and available for property defaults / forward references.
    let out = run_php(
        r#"<?php
namespace App\Config;
const VERSION = 42;
echo VERSION;
"#,
    );
    assert_eq!(out, "42");
}

#[test]
fn test_const_in_namespace_used_by_class() {
    // Constant inside namespace pre-scanned so class property default can reference it.
    let out = run_php(
        r#"<?php
namespace App;
const MAX = 100;

class Config {
    public $limit = MAX;
}

$c = new Config();
echo $c->limit;
"#,
    );
    assert_eq!(out, "100");
}

#[test]
fn test_global_and_source_magic_constants() {
    let out = run_php_with_source_context(
        "<?php\necho __LINE__ . '|' . __FILE__ . '|' . __DIR__ . '|' . __file__;",
        "/virtual/project/example.php",
        "/virtual/project",
    );
    assert_eq!(
        out,
        "2|/virtual/project/example.php|/virtual/project|/virtual/project/example.php"
    );
}

#[test]
fn public_php_platform_identity_is_consistent() {
    let output = run_php(
        r#"<?php
echo PHP_MAJOR_VERSION, ".", PHP_MINOR_VERSION, ".", PHP_RELEASE_VERSION, "|";
echo PHP_VERSION_ID, "|", PHP_VERSION, "|", phpversion(), "|";
echo PHP_INT_SIZE, "|", PHP_SAPI, "|", php_sapi_name(), "|";
var_dump(phpversion("missing"), extension_loaded("missing"));
"#,
    );

    assert_eq!(
        output,
        "8.2.0|80200|8.2.0|8.2.0|8|cli|cli|bool(false)\nbool(false)\n"
    );
}

#[test]
fn source_magic_constants_are_available_in_declaration_defaults() {
    let out = run_php_with_source_context(
        r#"<?php
const SOURCE_ROOT = __DIR__;
class GeneratedPaths {
    public static $files = [
        'main' => __DIR__ . '/src/main.php',
        'self' => __FILE__,
    ];
}
echo SOURCE_ROOT . '|';
echo GeneratedPaths::$files['main'] . '|';
echo GeneratedPaths::$files['self'];
"#,
        "/virtual/project/generated.php",
        "/virtual/project",
    );
    assert_eq!(
        out,
        "/virtual/project|/virtual/project/src/main.php|/virtual/project/generated.php"
    );
}

#[test]
fn test_scope_magic_constants() {
    let out = run_php(
        r#"<?php
namespace Demo;
function probe() {
    echo __FUNCTION__ . '|' . __METHOD__ . '|' . __CLASS__ . '|' . __NAMESPACE__ . ';';
}
class Subject {
    public function probe() {
        echo __FUNCTION__ . '|' . __METHOD__ . '|' . __CLASS__ . '|' . __TRAIT__ . '|' . __NAMESPACE__ . ';';
    }
}
trait NamedTrait {
    public function traitProbe() {
        echo __FUNCTION__ . '|' . __METHOD__ . '|' . __TRAIT__ . ';';
    }
}
class UsesTrait { use NamedTrait; }
probe();
(new Subject())->probe();
(new UsesTrait())->traitProbe();
"#,
    );
    assert_eq!(
        out,
        "Demo\\probe|Demo\\probe||Demo;probe|Demo\\Subject::probe|Demo\\Subject||Demo;traitProbe|Demo\\NamedTrait::traitProbe|Demo\\NamedTrait;"
    );
}

#[test]
fn test_fully_qualified_builtin_constant_uses_global_lookup() {
    assert_eq!(
        run_php("<?php echo \\PHP_EOL === PHP_EOL ? 'yes' : 'no';"),
        "yes"
    );
}
