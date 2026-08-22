/// E2E tests: stdlib functions — count, strlen, array_*, string functions, math, type checks.
mod common;
use common::{run_php, run_php_with_source_context};

#[test]
fn last_error_tracks_unhandled_diagnostics_even_when_suppressed() {
    assert_eq!(
        run_php(
            r#"<?php
var_dump(error_get_last());
@trigger_error('hidden', E_USER_NOTICE);
$hidden = error_get_last();
echo $hidden['type'], ':', $hidden['message'], ':', $hidden['line'], "\n";
set_error_handler(function ($level, $message) { echo "handled:$message\n"; return true; });
trigger_error('claimed', E_USER_WARNING);
echo error_get_last()['message'], "\n";
restore_error_handler();
error_clear_last();
var_dump(error_get_last());
"#,
        ),
        "NULL\n1024:hidden:3\nhandled:claimed\nhidden\nNULL\n"
    );
}

#[test]
fn strstr_is_binary_safe_and_supports_the_before_needle_boundary() {
    assert_eq!(
        run_php(
            r#"<?php
foreach ([
    ["left:right:right", ":", false],
    ["left:right:right", ":", true],
    ["abc", "", false],
    ["abc", "", true],
    ["a\0b", "\0", false],
    ["a\0b", "\0", true],
    ["abc", "missing", false],
] as [$haystack, $needle, $before]) {
    var_dump(strstr($haystack, $needle, $before));
}
"#,
        ),
        "string(12) \":right:right\"\nstring(4) \"left\"\nstring(3) \"abc\"\nstring(0) \"\"\nstring(2) \"\0b\"\nstring(1) \"a\"\nbool(false)\n"
    );
}

#[test]
fn json_preserve_zero_fraction_constant_matches_php_85() {
    assert_eq!(run_php("<?php echo JSON_PRESERVE_ZERO_FRACTION;"), "1024");
}

#[test]
fn random_interval_boundary_exposes_the_native_unit_enum_contract() {
    assert_eq!(
        run_php(
            r#"<?php
use Random\IntervalBoundary;

echo (int) enum_exists(IntervalBoundary::class), ":",
    (int) class_exists(IntervalBoundary::class), "\n";
foreach (IntervalBoundary::cases() as $case) {
    echo $case->name, ":",
        (int) ($case === constant(IntervalBoundary::class . "::" . $case->name)), ":",
        (int) ($case instanceof UnitEnum), "\n";
}
$class = new ReflectionClass(IntervalBoundary::class);
$method = new ReflectionMethod(IntervalBoundary::class, "cases");
echo (int) $class->isInternal(), ":", (int) $class->isFinal(), ":",
    (int) $method->isStatic(), ":", $method->getReturnType()->getName();
"#,
        ),
        concat!(
            "1:1\n",
            "ClosedOpen:1:1\n",
            "ClosedClosed:1:1\n",
            "OpenClosed:1:1\n",
            "OpenOpen:1:1\n",
            "1:0:1:array",
        )
    );
}

#[test]
fn assert_callable_uses_boolean_result_description_and_global_namespace_fallback() {
    assert_eq!(
        run_php(
            r#"<?php
namespace Example;
$check = assert(...);
var_dump($check(true));
foreach ([[false], [false, "reason"]] as $arguments) {
    try { $check(...$arguments); }
    catch (\AssertionError $error) { echo "[", $error->getMessage(), "]"; }
}

try { $check(false, new \Exception("preserved")); }
catch (\Throwable $error) { echo "[", get_class($error), ":", $error->getMessage(), "]"; }
try { $check(false, []); }
catch (\TypeError $error) { echo "[", $error->getMessage(), "]"; }
"#,
        ),
        "bool(true)\n[][reason][Exception:preserved][assert(): Argument #2 ($description) must be of type Throwable|string|null, array given]"
    );
}

#[test]
fn direct_assert_uses_php_85_canonical_expression_as_default_description() {
    assert_eq!(
        run_php(
            r#"<?php
namespace Example;
foreach ([1, 2] as $value) {
    try { assert(false && $value < 3 |> (fn() => 4)); }
    catch (\AssertionError $error) { echo $error->getMessage(), "\n"; }
}

"#,
        ),
        "assert(false && $value < 3 |> (fn() => 4))\nassert(false && $value < 3 |> (fn() => 4))\n"
    );
}

#[test]
fn assert_source_preserves_float_compound_assignment_and_exit_canonicalization() {
    assert_eq!(
        run_php(
            r#"<?php
function printAssertion(Throwable $error) { echo $error->getMessage(), "\n--\n"; }
try { assert(!is_float(2.0)); }
catch (Throwable $error) { printAssertion($error); }
try { $number = 3; assert(false && ($number **= 4)); }
catch (Throwable $error) { printAssertion($error); }
try { assert(false && exit("unreached")); }
catch (Throwable $error) { printAssertion($error); }
"#,
        ),
        "assert(!is_float(2.0))\n--\nassert(false && ($number **= 4))\n--\nassert(false && \\exit('unreached'))\n--\n"
    );
}

#[test]
fn assert_source_formats_multiline_closures_match_and_property_visibility() {
    assert_eq!(
        run_php(
            r#"<?php
function printAssertion(Throwable $error) { echo $error->getMessage(), "\n--\n"; }
try { assert((function () { return false; })()); }
catch (Throwable $error) { printAssertion($error); }
try {
    assert((function () {
        match ('other') {
            'chosen' => true,
            default => false,
        };
    })());
} catch (Throwable $error) { printAssertion($error); }
try {
    assert(function () {
        class LocalContract {
            public private(set) int $value;
        }
    } && false);
} catch (Throwable $error) { printAssertion($error); }
"#,
        ),
        "assert((function () {\n    return false;\n})())\n--\nassert((function () {\n    match ('other') {\n        'chosen' => true,\n        default => false,\n    };\n})())\n--\nassert(function () {\n    class LocalContract {\n        public private(set) int $value;\n    }\n\n} && false)\n--\n"
    );
}

#[test]
fn assert_source_formats_typed_class_constants_without_running_the_class_body() {
    assert_eq!(
        run_php(
            r#"<?php
interface LeftContract {}
interface RightContract {}
function printAssertion(Throwable $error) { echo $error->getMessage(), "\n--\n"; }

try {
    assert(false && new class {
        final protected const string LABEL = 'value';
        private const float RATE = 1.5;
        public const (LeftContract&RightContract)|null VALUE = null;
    });
} catch (Throwable $error) { printAssertion($error); }

try {
    assert(function () {
        class LocalConstants {
            const ENABLED = true;
        }
    } && false);
} catch (Throwable $error) { printAssertion($error); }
"#,
        ),
        concat!(
            "assert(false && new class {\n",
            "    protected const string LABEL = 'value';\n",
            "    private const float RATE = 1.5;\n",
            "    public const LeftContract&RightContract|null VALUE = null;\n",
            "})\n--\n",
            "assert(function () {\n",
            "    class LocalConstants {\n",
            "        public const ENABLED = true;\n",
            "    }\n\n",
            "} && false)\n--\n",
        )
    );
}

#[test]
fn assert_options_preserve_request_local_state_and_callback_contract() {
    assert_eq!(
        run_php(
            r#"<?php
@assert_options(ASSERT_EXCEPTION, 0);
@assert_options(ASSERT_WARNING, 0);
echo assert_options(ASSERT_ACTIVE), ":", assert_options(ASSERT_BAIL, 1), ":", assert_options(ASSERT_BAIL, 0), "\n";
@assert_options(ASSERT_CALLBACK, function($file, $line, $code, $description = null) {
    var_dump($file !== "", is_int($line), $code === null, $description);
});
var_dump(assert(false));
var_dump(assert_options(ASSERT_CALLBACK, null) instanceof Closure);
var_dump(assert_options(ASSERT_CALLBACK));
@assert_options(ASSERT_ACTIVE, 0);
var_dump(assert(false));
"#,
        ),
        "1:0:1\nbool(true)\nbool(true)\nbool(true)\nstring(13) \"assert(false)\"\nbool(false)\nbool(true)\nNULL\nbool(true)\n"
    );
}

#[test]
fn strict_internal_string_calls_reject_scalars_without_weakening_ordinary_calls() {
    assert_eq!(
        run_php("<?php echo strlen(1.5), ':', ord(65), \"\\n\";"),
        "3:54\n"
    );
    assert_eq!(
        run_php(
            r#"<?php
declare(strict_types=1);
foreach ([["strlen", 1.5], ["ord", 65]] as [$function, $argument]) {
    try { $function($argument); }
    catch (TypeError $error) { echo $error->getMessage(), "\n"; }
}
try { array_map([null, "method"], []); }
catch (TypeError $error) { echo $error->getMessage(); }
"#,
        ),
        "strlen(): Argument #1 ($string) must be of type string, float given\nord(): Argument #1 ($character) must be of type string, int given\narray_map(): Argument #1 ($callback) must be a valid callback or null, first array member is not a valid class name or object"
    );
}

#[test]
fn addcslashes_preserves_php_reference_escaping_rules() {
    assert_eq!(
        run_php(
            r#"<?php
echo addcslashes("az-A'\\", "a..z'\\"), '|';
$controls = chr(0).chr(7).chr(8).chr(9).chr(10).chr(11).chr(12).chr(13).chr(31).chr(127);
echo addcslashes($controls, chr(0).'..'.chr(127));
"#,
        ),
        "\\a\\z-A\\'\\\\|\\000\\a\\b\\t\\n\\v\\f\\r\\037\\177"
    );
}

// === count() ===

#[test]
fn test_e2e_count_array() {
    assert_eq!(run_php("<?php echo count([1, 2, 3]);"), "3");
}

#[test]
fn test_e2e_count_empty() {
    assert_eq!(run_php("<?php echo count([]);"), "0");
}

#[test]
fn test_e2e_count_assoc() {
    assert_eq!(run_php("<?php echo count(['a' => 1, 'b' => 2]);"), "2");
}

#[test]
fn test_e2e_count_null() {
    assert_eq!(
        run_php("<?php try { count(null); } catch (TypeError $e) { echo $e->getMessage(); }"),
        "count(): Argument #1 ($value) must be of type Countable|array, null given"
    );
}

#[test]
fn test_e2e_count_scalar() {
    assert_eq!(
        run_php("<?php try { count(42); } catch (TypeError $e) { echo $e->getMessage(); }"),
        "count(): Argument #1 ($value) must be of type Countable|array, int given"
    );
}

#[test]
fn array_search_supports_strict_identity_matching() {
    assert_eq!(
        run_php(
            "<?php $values = ['loose' => '0', 'strict' => 0]; echo array_search(0, $values), '|'; echo array_search(0, $values, true);"
        ),
        "loose|strict"
    );
}

#[test]
fn array_fill_keys_matches_php_85_keys_values_diagnostics_and_detachment() {
    assert_eq!(
        run_php(
            r#"<?php
class FillKey {
    public function __construct(public string $key) {}
    public function __toString(): string { echo "K:$this->key\n"; return $this->key; }
}
set_error_handler(function ($level, $message) {
    echo "$level:$message\n";
    return true;
});

$seed = ['seed'];
$alias = &$seed;
$result = array_fill_keys([
    1, '1', '01', -2, '-2', '-0', true, false, null, 1.25, NAN, [1],
    new FillKey('8'), new FillKey('08'),
], $alias);
var_dump(array_keys($result));
$seed[] = 'outside';
$result['01'][] = 'inside';
echo count($result['01']), '|', count($result[1]), '|', count($seed), "\n";

$object = new stdClass();
$objects = array_fill_keys(['left', 'right'], $object);
var_dump($objects['left'] === $objects['right']);

$key = 'first';
$keys = [&$key];
$detached = array_fill_keys($keys, 'value');
$key = 'changed';
var_dump(isset($detached['first']), isset($detached['changed']));

foreach ([null, true, new stdClass()] as $invalid) {
    try { array_fill_keys($invalid, 0); }
    catch (TypeError $error) { echo $error->getMessage(), "\n"; }
}

set_error_handler(function ($level, $message) {
    echo "throw:$message\n";
    throw new Exception('stopped');
});
try { array_fill_keys([[1], new FillKey('later')], 0); }
catch (Exception $error) { echo $error->getMessage(), "\n"; }
try { array_fill_keys([new stdClass()], 0); }
catch (Error $error) { echo $error->getMessage(), "\n"; }
"#,
        ),
        concat!(
            "2:unexpected NAN value was coerced to string\n",
            "2:Array to string conversion\n",
            "K:8\nK:08\n",
            "array(10) {\n",
            "  [0]=>\n  int(1)\n",
            "  [1]=>\n  string(2) \"01\"\n",
            "  [2]=>\n  int(-2)\n",
            "  [3]=>\n  string(2) \"-0\"\n",
            "  [4]=>\n  string(0) \"\"\n",
            "  [5]=>\n  string(4) \"1.25\"\n",
            "  [6]=>\n  string(3) \"NAN\"\n",
            "  [7]=>\n  string(5) \"Array\"\n",
            "  [8]=>\n  int(8)\n",
            "  [9]=>\n  string(2) \"08\"\n",
            "}\n",
            "2|1|2\n",
            "bool(true)\n",
            "bool(true)\n",
            "bool(false)\n",
            "array_fill_keys(): Argument #1 ($keys) must be of type array, null given\n",
            "array_fill_keys(): Argument #1 ($keys) must be of type array, true given\n",
            "array_fill_keys(): Argument #1 ($keys) must be of type array, stdClass given\n",
            "throw:Array to string conversion\n",
            "stopped\n",
            "Object of class stdClass could not be converted to string\n",
        )
    );
}

#[test]
fn debug_backtrace_reports_callers_arguments_limits_and_method_receivers() {
    assert_eq!(
        run_php(
            r#"<?php
function traceOuter($value) { traceInner($value); }
function traceInner($value) {
    $trace = debug_backtrace();
    echo count($trace), ':', $trace[0]['function'], ':', $trace[0]['args'][0], ':';
    echo $trace[1]['function'], ':', $trace[1]['args'][0], '|';
    $limited = debug_backtrace(DEBUG_BACKTRACE_IGNORE_ARGS, 1);
    echo count($limited), ':', isset($limited[0]['args']) ? 'args' : 'ignored';
}

class TraceReceiver {
    public function probe() {
        $trace = debug_backtrace(DEBUG_BACKTRACE_PROVIDE_OBJECT, 1);
        echo '|', $trace[0]['class'], ':', $trace[0]['function'], ':', $trace[0]['type'], get_class($trace[0]['object']);
    }
}
traceOuter('payload');
(new TraceReceiver())->probe();
"#,
        ),
        "2:traceInner:payload:traceOuter:payload|1:ignored|TraceReceiver:probe:->TraceReceiver"
    );
}

#[test]
fn debug_print_backtrace_formats_locations_arguments_limits_and_main() {
    assert_eq!(
        run_php_with_source_context(
            "<?php\nfunction outer($value) { inner($value); }\nfunction inner($value) { debug_print_backtrace(); }\nouter('payload');",
            "/app/trace.php",
            "/app",
        ),
        "#0 /app/trace.php(2): inner('payload')\n#1 /app/trace.php(4): outer('payload')\n"
    );
    assert_eq!(
        run_php_with_source_context(
            "<?php\nfunction outer($value) { inner($value); }\nfunction inner($value) { debug_print_backtrace(DEBUG_BACKTRACE_IGNORE_ARGS, 1); }\nouter('payload');",
            "/app/trace.php",
            "/app",
        ),
        "#0 /app/trace.php(2): inner()\n"
    );
    assert_eq!(
        run_php("<?php var_dump(debug_print_backtrace());"),
        "NULL\n"
    );
}

#[test]
fn argument_introspection_uses_live_parameters_and_retains_only_extra_values() {
    assert_eq!(
        run_php(
            r#"<?php
function caller_arguments() {
    return debug_backtrace()[1]['args'];
}
function mutate_arguments($fixed, &$changed, $removed) {
    echo json_encode(caller_arguments()), '|';
    $changed = 'after';
    unset($removed);
    echo json_encode(func_get_args()), '|', json_encode(caller_arguments()), '|';
}
$changed = 'before';
mutate_arguments('fixed', $changed, 'gone', 'extra');
echo $changed, '|';

class MagicArgumentSnapshot {
    public function __call($name, $arguments) {
        eval('$arguments = []; echo json_encode(debug_backtrace()[1]["args"]), "|";');
        return debug_backtrace()[0]['args'];
    }
}
echo json_encode((new MagicArgumentSnapshot)->missing('value'));
"#,
        ),
        concat!(
            r#"["fixed","before","gone","extra"]|"#,
            r#"["fixed","after",null,"extra"]|"#,
            r#"["fixed","after",null,"extra"]|after|"#,
            r#"["missing",[]]|"#,
            r#"["missing",[]]"#,
        )
    );
}

#[test]
fn sensitive_parameter_redacts_live_and_throwable_traces() {
    assert_eq!(
        run_php_with_source_context(
            r#"<?php
function captured($plain, #[SensitiveParameter] $secret, $tail = 'tail') {
    $live = debug_backtrace()[0]['args'];
    $thrown = (new Exception)->getTrace()[0]['args'];
    echo $live[0], ':', get_class($live[1]), ':', $live[1]->getValue(), ':', $live[2], '|';
    echo $thrown[0], ':', get_class($thrown[1]), ':', $thrown[1]->getValue(), ':', $thrown[2], '|';
}
captured('plain', 'secret', 'tail', 'extra');

function named(#[SensitiveParameter] $first = null, $plain = null, #[SensitiveParameter] $last = null) {
    $args = debug_backtrace()[0]['args'];
    echo count($args), ':', get_class($args[0]), ':', get_class($args[2]), ':', $args[2]->getValue(), '|';
}
named(plain: 'plain', last: 'last');

function variadic($plain, #[SensitiveParameter] ...$secret) {
    return (new Exception)->getTrace()[0]['args'];
}
$args = variadic('plain', 'one', 'two');
echo $args[0], ':', get_class($args[1]), ':', $args[1]->getValue(), ':', get_class($args[2]), ':', $args[2]->getValue();
"#,
            "/app/sensitive-trace.php",
            "/app",
        ),
        concat!(
            "plain:SensitiveParameterValue:secret:tail|",
            "plain:SensitiveParameterValue:secret:tail|",
            "3:SensitiveParameterValue:SensitiveParameterValue:last|",
            "plain:SensitiveParameterValue:one:SensitiveParameterValue:two",
        )
    );
}

#[test]
fn sensitive_parameter_formats_debug_print_backtrace_without_disclosure() {
    assert_eq!(
        run_php_with_source_context(
            "<?php\nfunction concealed(#[SensitiveParameter] $secret) { debug_print_backtrace(); }\nconcealed('do-not-print');",
            "/app/sensitive.php",
            "/app",
        ),
        "#0 /app/sensitive.php(3): concealed(Object(SensitiveParameterValue))\n"
    );
    assert_eq!(
        run_php_with_source_context(
            "<?php\nfunction concealedNamed($plain, #[SensitiveParameter] ...$secret) { debug_print_backtrace(); }\nconcealedNamed(plain: 2, first: 'one', second: 'two');",
            "/app/sensitive-named.php",
            "/app",
        ),
        "#0 /app/sensitive-named.php(3): concealedNamed(2, first: Object(SensitiveParameterValue), second: Object(SensitiveParameterValue))\n"
    );
}

#[test]
fn sensitive_parameter_value_is_opaque_but_retains_its_value() {
    assert_eq!(
        run_php(
            r#"<?php
$value = new SensitiveParameterValue('secret');
var_dump($value);
debug_zval_dump($value);
print_r($value);
var_dump([$value]);
echo var_export($value, true), '|', count((array) $value), ':', json_encode($value), '|';
echo $value->getValue(), ':', (clone $value)->getValue(), '|';
try { serialize($value); } catch (Throwable $error) { echo get_class($error), ':', $error->getMessage(), '|'; }
try { echo (string) $value; } catch (Throwable $error) { echo get_class($error), ':', $error->getMessage(), '|'; }
try { $value->dynamic = 1; } catch (Throwable $error) { echo get_class($error), ':', $error->getMessage(); }
"#,
        ),
        concat!(
            "object(SensitiveParameterValue)#1 (0) {\n}\n",
            "object(SensitiveParameterValue)#1 (0) refcount(2){\n}\n",
            "SensitiveParameterValue Object\n(\n)\n",
            "array(1) {\n  [0]=>\n  object(SensitiveParameterValue)#1 (0) {\n  }\n}\n",
            "\\SensitiveParameterValue::__set_state(array(\n))|0:{}|",
            "secret:secret|",
            "Exception:Serialization of 'SensitiveParameterValue' is not allowed|",
            "Error:Object of class SensitiveParameterValue could not be converted to string|",
            "Error:Cannot create dynamic property SensitiveParameterValue::$dynamic",
        )
    );
}

#[test]
fn sensitive_parameter_trace_snapshot_keeps_an_object_alive() {
    assert_eq!(
        run_php_with_source_context(
            r#"<?php
class RetainedSecret {
    public function __destruct() { echo 'destroyed|'; }
}
function retain(#[SensitiveParameter] $secret) {
    return (new Exception)->getTrace()[0]['args'][0];
}
$wrapper = retain(new RetainedSecret());
echo get_class($wrapper), ':', get_class($wrapper->getValue()), '|alive|';
unset($wrapper);
echo 'released';
"#,
            "/app/sensitive-lifetime.php",
            "/app",
        ),
        "SensitiveParameterValue:RetainedSecret|alive|releaseddestroyed|"
    );
}

// === strlen() ===

#[test]
fn test_e2e_strlen_basic() {
    assert_eq!(run_php("<?php echo strlen('hello');"), "5");
}

#[test]
fn test_e2e_strlen_empty() {
    assert_eq!(run_php("<?php echo strlen('');"), "0");
}

#[test]
fn test_e2e_strlen_number() {
    assert_eq!(run_php("<?php echo strlen(12345);"), "5");
}

#[test]
fn null_scalar_builtin_contracts_report_and_throw_like_php_82() {
    assert_eq!(
        run_php_with_source_context(
            r#"<?php
set_error_handler(function($level, $message, $file, $line) { echo "$level:$message:$line\n"; });
$null = null;
var_dump(strlen($null));
var_dump(ord($null));
var_dump(defined($null));
var_dump(chr($null));
var_dump(array_slice(['x'], $null, $null));
restore_error_handler();
foreach ([
    fn() => get_class($null),
    fn() => array_slice($null, 0),
    fn() => array_key_exists('x', $null),
] as $call) {
    try { $call(); } catch (TypeError $error) { echo $error->getMessage(), "\n"; }
}
"#,
            "/virtual/null-builtins.php",
            "/virtual",
        ),
        concat!(
            "8192:strlen(): Passing null to parameter #1 ($string) of type string is deprecated:4\n",
            "int(0)\n",
            "8192:ord(): Passing null to parameter #1 ($character) of type string is deprecated:5\n",
            "int(0)\n",
            "8192:defined(): Passing null to parameter #1 ($constant_name) of type string is deprecated:6\n",
            "bool(false)\n",
            "8192:chr(): Passing null to parameter #1 ($codepoint) of type int is deprecated:7\n",
            "string(1) \"\0\"\n",
            "8192:array_slice(): Passing null to parameter #2 ($offset) of type int is deprecated:8\n",
            "array(1) {\n  [0]=>\n  string(1) \"x\"\n}\n",
            "get_class(): Argument #1 ($object) must be of type object, null given\n",
            "array_slice(): Argument #1 ($array) must be of type array, null given\n",
            "array_key_exists(): Argument #2 ($array) must be of type array, null given\n",
        )
    );
}

// === substr() ===

#[test]
fn test_e2e_substr_basic() {
    assert_eq!(run_php("<?php echo substr('hello world', 6);"), "world");
}

#[test]
fn test_e2e_substr_with_length() {
    assert_eq!(run_php("<?php echo substr('hello world', 0, 5);"), "hello");
}

#[test]
fn test_e2e_substr_negative_start() {
    assert_eq!(run_php("<?php echo substr('hello', -3);"), "llo");
}

// === strpos() ===

#[test]
fn test_e2e_strpos_found() {
    assert_eq!(run_php("<?php echo strpos('hello world', 'world');"), "6");
}

#[test]
fn test_e2e_strpos_not_found() {
    // Returns false, which echoes as empty string
    assert_eq!(
        run_php("<?php $r = strpos('hello', 'xyz'); if (!$r) { echo 'not found'; }"),
        "not found"
    );
}

#[test]
fn strpos_supports_positive_and_negative_offsets() {
    assert_eq!(
        run_php("<?php var_dump(strpos('abcabc', 'a', 1)); var_dump(strpos('abcabc', 'a', -4));"),
        "int(3)\nint(3)\n"
    );
}

#[test]
fn strrchr_returns_the_suffix_at_the_last_first_byte_match() {
    assert_eq!(
        run_php(
            "<?php echo strrchr('path/to/file.php', '/'), '|'; echo strrchr('abcabc', 'cd'), '|'; var_dump(strrchr('abc', ''));"
        ),
        "/file.php|c|bool(false)\n"
    );
}

// === strtr() ===

#[test]
fn test_e2e_strtr_character_map_is_simultaneous_and_truncates_to_shorter_map() {
    assert_eq!(
        run_php("<?php echo strtr('hello', 'el', 'ip'), '|'; echo strtr('abcd', 'abc', 'X');"),
        "hippo|Xbcd"
    );
}

#[test]
fn test_e2e_strtr_pair_map_prefers_longest_key_and_accepts_integer_keys() {
    assert_eq!(
        run_php(
            "<?php echo strtr('ababa', ['ab' => 'X', 'a' => 'Y']), '|'; echo strtr('123', [1 => 'x', '12' => 'y']);"
        ),
        "XXY|y3"
    );
}

#[test]
fn test_e2e_strtr_pair_map_warns_and_ignores_empty_keys() {
    assert_eq!(
        run_php("<?php echo strtr('abc', ['' => 'x']);"),
        "Warning: strtr(): Ignoring replacement of empty string\nabc"
    );
}

// === str_replace() ===

#[test]
fn test_e2e_str_replace() {
    assert_eq!(
        run_php("<?php echo str_replace('world', 'PHP', 'hello world');"),
        "hello PHP"
    );
}

#[test]
fn test_e2e_str_replace_multiple() {
    assert_eq!(
        run_php("<?php echo str_replace('o', '0', 'foo bar boo');"),
        "f00 bar b00"
    );
}

#[test]
fn str_replace_supports_array_pairs_subject_keys_and_count() {
    assert_eq!(
        run_php(
            r#"<?php
$value = str_replace(['/','+'], ['.','_'], 'a/b+c', $count);
echo "$value|$count\n";
$values = str_replace(['a','b'], [2 => 'X', 1 => 'Y'], ['k' => 'aba', -1 => 'cab'], $count);
echo $values['k'], '|', $values[-1], '|', $count, "\n";
echo str_replace(['a', 'b'], ['b', 'c'], 'a'), "\n";
echo str_replace('', 'X', 'ab', $count), '|', $count;
"#,
        ),
        "a.b_c|2\nXYX|cXY|5\nc\nab|0"
    );
}

#[test]
fn str_replace_rejects_array_replacement_for_scalar_search() {
    assert_eq!(
        run_php(
            "<?php try { str_replace('a', ['X'], 'a'); } catch (TypeError $error) { echo $error->getMessage(); }"
        ),
        "str_replace(): Argument #2 ($replace) must be of type string when argument #1 ($search) is a string"
    );
}

// === strtolower / strtoupper ===

#[test]
fn test_e2e_strtolower() {
    assert_eq!(
        run_php("<?php echo strtolower('HELLO World');"),
        "hello world"
    );
}

#[test]
fn test_e2e_strtoupper() {
    assert_eq!(
        run_php("<?php echo strtoupper('hello World');"),
        "HELLO WORLD"
    );
}

// === trim() ===

#[test]
fn test_e2e_trim() {
    assert_eq!(run_php("<?php echo trim('  hello  ');"), "hello");
}

#[test]
fn test_e2e_trim_character_mask_and_range() {
    assert_eq!(
        run_php(
            "<?php echo ltrim('/route/', '/'), '|', rtrim('/route/', '/'), '|', trim('012abc210', '0..2');"
        ),
        "route/|/route|abc"
    );
}

// === explode / implode ===

#[test]
fn test_e2e_explode() {
    assert_eq!(
        run_php(
            "<?php $parts = explode(',', 'a,b,c'); echo $parts[0]; echo $parts[1]; echo $parts[2];"
        ),
        "abc"
    );
}

#[test]
fn test_e2e_implode() {
    assert_eq!(run_php("<?php echo implode('-', [1, 2, 3]);"), "1-2-3");
}

#[test]
fn test_e2e_implode_mixed_scalar_values() {
    assert_eq!(
        run_php("<?php echo implode('|', [null, false, true, 42, 1.5, 'x']);"),
        "||1|42|1.5|x"
    );
}

#[test]
fn test_e2e_chain_scalar_stdlib() {
    // Chain two scalar stdlib calls
    assert_eq!(run_php("<?php $x = strlen('hello'); echo $x;"), "5");
    assert_eq!(
        run_php("<?php $x = strlen('hello'); $y = strlen('world'); echo $x; echo $y;"),
        "55"
    );
}

#[test]
fn test_e2e_array_to_count_inline() {
    // Pass array literal directly to count — works
    assert_eq!(run_php("<?php $a = [1, 2, 3]; echo count($a);"), "3");
}

#[test]
fn test_e2e_explode_then_count() {
    assert_eq!(
        run_php("<?php $parts = explode(' ', 'a b c'); echo count($parts);"),
        "3"
    );
}

#[test]
fn test_e2e_explode_implode_round_trip() {
    assert_eq!(
        run_php("<?php $parts = explode(' ', 'a b c'); echo implode('_', $parts);"),
        "a_b_c"
    );
}

// === str_repeat ===

#[test]
fn test_e2e_str_repeat() {
    assert_eq!(
        run_php(
            "<?php echo str_repeat('ab', 3), '|'; try { str_repeat('x', -1); } catch (ValueError $error) { echo $error->getMessage(); } echo '|', str_repeat('', PHP_INT_MAX);"
        ),
        "ababab|str_repeat(): Argument #2 ($times) must be greater than or equal to 0|"
    );
}

#[test]
fn strrev_is_registered_with_php_name_named_argument_and_scalar_coercion() {
    assert_eq!(
        run_php(
            "<?php echo strrev('stressed'), '|', strrev(string: 'drawer'), '|', strrev(120), '|'; var_dump(function_exists('str_rev'));"
        ),
        "desserts|reward|021|bool(false)\n"
    );
}

// === substr_count ===

#[test]
fn test_e2e_substr_count() {
    assert_eq!(
        run_php("<?php echo substr_count('hello world hello', 'hello');"),
        "2"
    );
}

// === str_contains / str_starts_with / str_ends_with (PHP 8) ===

#[test]
fn test_e2e_str_contains_true() {
    assert_eq!(
        run_php("<?php echo str_contains('hello world', 'world') ? 'yes' : 'no';"),
        "yes"
    );
}

#[test]
fn test_e2e_str_contains_false() {
    assert_eq!(
        run_php("<?php echo str_contains('hello world', 'xyz') ? 'yes' : 'no';"),
        "no"
    );
}

#[test]
fn test_e2e_str_starts_with() {
    assert_eq!(
        run_php("<?php echo str_starts_with('hello world', 'hello') ? 'yes' : 'no';"),
        "yes"
    );
}

#[test]
fn test_e2e_str_ends_with() {
    assert_eq!(
        run_php("<?php echo str_ends_with('hello world', 'world') ? 'yes' : 'no';"),
        "yes"
    );
}

// === array_reverse ===
// NOTE: array_push/array_pop/array_shift require pass-by-reference semantics
// which we don't support yet. Those are registered but will be testable once
// we implement &$param support.

// === array_key_exists / in_array ===

#[test]
fn test_e2e_array_key_exists_true() {
    assert_eq!(
        run_php(
            "<?php $a = ['name' => 'Alice']; echo array_key_exists('name', $a) ? 'yes' : 'no';"
        ),
        "yes"
    );
}

#[test]
fn test_e2e_array_key_exists_false() {
    assert_eq!(
        run_php("<?php $a = ['name' => 'Alice']; echo array_key_exists('age', $a) ? 'yes' : 'no';"),
        "no"
    );
}

#[test]
fn test_e2e_in_array_found() {
    assert_eq!(
        run_php("<?php echo in_array(2, [1, 2, 3]) ? 'yes' : 'no';"),
        "yes"
    );
}

#[test]
fn test_e2e_in_array_not_found() {
    assert_eq!(
        run_php("<?php echo in_array(5, [1, 2, 3]) ? 'yes' : 'no';"),
        "no"
    );
}

#[test]
fn test_e2e_in_array_string() {
    assert_eq!(
        run_php("<?php echo in_array('b', ['a', 'b', 'c']) ? 'yes' : 'no';"),
        "yes"
    );
}

#[test]
fn test_e2e_in_array_strict_does_not_coerce() {
    assert_eq!(
        run_php("<?php echo in_array(1, ['1'], true) ? 'bad' : 'strict';"),
        "strict"
    );
}

#[test]
fn test_filter_var_common_validators() {
    assert_eq!(
        run_php(
            "<?php echo filter_var('42', FILTER_VALIDATE_INT), ':'; echo filter_var('yes', FILTER_VALIDATE_BOOL) ? 'true' : 'false'; echo ':'; echo filter_var('127.0.0.1', FILTER_VALIDATE_IP, FILTER_FLAG_IPV4); echo ':'; echo filter_var('bad', FILTER_VALIDATE_INT, FILTER_NULL_ON_FAILURE) === null ? 'null' : 'bad';"
        ),
        "42:true:127.0.0.1:null"
    );
}

#[test]
fn filter_var_validates_floats() {
    assert_eq!(
        run_php(
            "<?php echo filter_var('1.25', FILTER_VALIDATE_FLOAT), ':'; echo filter_var('not-a-float', FILTER_VALIDATE_FLOAT) === false ? 'false' : 'bad';"
        ),
        "1.25:false"
    );
}

#[test]
fn output_buffer_level_starts_at_zero() {
    assert_eq!(run_php("<?php echo ob_get_level();"), "0");
}

#[test]
fn output_buffers_nest_and_return_raw_contents() {
    assert_eq!(
        run_php(
            "<?php ob_start(); echo 'A'; ob_start(); echo 'B'; $inner = ob_get_clean(); echo 'C'; $outer = ob_get_clean(); echo $inner, '|', $outer, '|', ob_get_level();"
        ),
        "B|AC|0"
    );
}

#[test]
fn output_buffer_callbacks_observe_start_clean_flush_and_final_phases() {
    assert_eq!(
        run_php(
            r#"<?php
function decorate($contents, $phase) { return '['.$phase.':'.$contents.']'; }
ob_start('decorate');
echo 'A';
ob_flush();
echo 'B';
ob_clean();
echo 'C';
ob_end_flush();
"#,
        ),
        "[5:A][8:C]"
    );
}

#[test]
fn output_buffer_method_callback_flushes_at_request_end() {
    assert_eq!(
        run_php(
            r#"<?php
class BufferOwner {
    public function __construct() { ob_start([$this, 'decorate']); }
    public function decorate($contents) { return strtoupper($contents); }
}
new BufferOwner();
echo 'success';
"#,
        ),
        "SUCCESS"
    );
}

#[test]
fn output_buffer_get_clean_returns_contents_when_flags_prevent_removal() {
    assert_eq!(
        run_php(
            "<?php ob_start(); ob_start(null, 0, 0); echo 'x'; $value = ob_get_clean(); echo '|', $value, '|', ob_get_level();"
        ),
        "x|x|2"
    );
}

#[test]
fn get_debug_type_reports_scalar_and_object_names() {
    assert_eq!(
        run_php(
            "<?php class DebugTypeObject {} echo get_debug_type(null) . ':' . get_debug_type(1) . ':' . get_debug_type(new DebugTypeObject()) . ':' . get_debug_type(static fn () => null);"
        ),
        "null:int:DebugTypeObject:Closure"
    );
}

#[test]
fn substr_compare_supports_offsets_lengths_and_case_folding() {
    assert_eq!(
        run_php(
            "<?php echo substr_compare('FrameworkBundle', 'workbench', 5, 4), ':'; echo substr_compare('Symfony', 'SYMFONY', 0, null, true), ':'; echo substr_compare('abc', 'abd', 0), ':'; echo substr_compare('abc', 'a', -4);"
        ),
        "0:0:-1:1"
    );
}

#[test]
fn strnatcmp_orders_numeric_segments_like_php() {
    assert_eq!(
        run_php(
            "<?php $values = ['img10', 'img2', 'img02', 'img1']; usort($values, 'strnatcmp'); echo implode(',', $values), ':'; echo strnatcmp('02', '2'), ':', strnatcmp('1.10', '1.2');"
        ),
        "img02,img1,img2,img10:0:1"
    );
}

#[test]
fn string_comparisons_preserve_byte_differences_limits_and_ascii_case_folding() {
    assert_eq!(
        run_php(
            r#"<?php
echo strcmp("a", "d"), ":", strcmp("qwe", "qwer"), ":", strcmp("A\x00B", "A\x00c"), "\n";
echo strcasecmp("qwerty", "QweRty"), ":", strcasecmp("A\x00B", "a\x00c"), "\n";
echo strncmp("qwerty", "qwerty123", 6), ":", strncmp("qwerty", "qwerty123", 7), ":", strncmp("a", "d", 0), "\n";
echo strncasecmp("qwErtY", "qwer", 7), ":", strncasecmp("q123", "Q123", 3), "\n";
echo strcmp(string2: "b", string1: "a"), "\n";
foreach (["strncmp", "strncasecmp"] as $function) {
    try { $function("a", "b", -1); } catch (ValueError $error) { echo $error->getMessage(), "\n"; }
}
"#,
        ),
        "-3:-1:-33\n0:-1\n0:-1:0\n1:0\n-1\nstrncmp(): Argument #3 ($length) must be greater than or equal to 0\nstrncasecmp(): Argument #3 ($length) must be greater than or equal to 0\n"
    );
}

#[test]
fn property_exists_sees_declared_static_inherited_trait_and_dynamic_properties() {
    assert_eq!(
        run_php(
            r#"<?php
trait TraitProperty { private $traitPrivate; }
class PropertyParent { private $parentPrivate; protected static $parentStatic; }
#[AllowDynamicProperties]
class PropertyChild extends PropertyParent { use TraitProperty; public int $uninitialized; }
$object = new PropertyChild;
$object->dynamic = 1;
foreach (["parentPrivate", "parentStatic", "traitPrivate", "uninitialized", "dynamic", "missing"] as $name) {
    var_dump(property_exists($object, $name));
}
unset($object->dynamic);
var_dump(property_exists($object, "dynamic"));
foreach (["parentPrivate", "parentStatic", "traitPrivate", "uninitialized", "missing"] as $name) {
    var_dump(property_exists("PropertyChild", $name));
}
var_dump(property_exists(function () {}, "anything"));
foreach ([[], 1, 3.5, true, null] as $invalid) {
    try { property_exists($invalid, "anything"); } catch (TypeError $error) { echo $error->getMessage(), "\n"; }
}
"#,
        ),
        concat!(
            "bool(true)\nbool(true)\nbool(true)\nbool(true)\nbool(true)\nbool(false)\n",
            "bool(false)\n",
            "bool(true)\nbool(true)\nbool(true)\nbool(true)\nbool(false)\n",
            "bool(false)\n",
            "property_exists(): Argument #1 ($object_or_class) must be of type object|string, array given\n",
            "property_exists(): Argument #1 ($object_or_class) must be of type object|string, int given\n",
            "property_exists(): Argument #1 ($object_or_class) must be of type object|string, float given\n",
            "property_exists(): Argument #1 ($object_or_class) must be of type object|string, bool given\n",
            "property_exists(): Argument #1 ($object_or_class) must be of type object|string, null given\n",
        )
    );
}

#[test]
fn incremental_xxh128_hash_matches_one_shot_hash() {
    assert_eq!(
        run_php(
            "<?php $context = hash_init('xxh128'); var_dump($context instanceof HashContext); hash_update($context, 'Symfony'); hash_update($context, '!'); echo hash_final($context), ':', hash('xxh128', 'Symfony!');"
        ),
        "bool(true)\n0290f3acc01ebe2ef48505b4a1179147:0290f3acc01ebe2ef48505b4a1179147"
    );
}

#[test]
fn array_diff_key_preserves_missing_keys_and_values() {
    assert_eq!(
        run_php(
            "<?php $result = array_diff_key(['keep' => 1, 'drop' => 2, 3 => 'three'], ['drop' => 9]); echo $result['keep'] . ':' . $result[3] . ':' . count($result);"
        ),
        "1:three:2"
    );
}

#[test]
fn array_diff_accepts_one_source_and_all_variadic_comparison_arrays() {
    assert_eq!(
        run_php(
            "<?php $source = ['first' => 1, 'second' => 2, 'third' => 3]; echo implode(',', array_diff($source)), '|', implode(',', array_diff($source, [2], [3]));"
        ),
        "1,2,3|1"
    );
}

#[test]
fn array_key_set_operations_accept_variadic_comparison_arrays() {
    assert_eq!(
        run_php(
            "<?php $source = ['keep' => 1, 'shared' => 2, 3 => 'three']; $diff = array_diff_key($source, ['shared' => 9], [3 => 'other']); $intersect = array_intersect_key($source, ['keep' => 0, 'shared' => 0], ['shared' => 0, 3 => 0]); echo count($diff), ':', $diff['keep'], ':', count($intersect), ':', $intersect['shared'];"
        ),
        "1:1:1:2"
    );
}

#[test]
fn array_is_list_checks_ordered_keys_without_requiring_packed_storage() {
    assert_eq!(
        run_php(
            "<?php var_dump(array_is_list([])); var_dump(array_is_list([0 => 'a', 1 => 'b'])); var_dump(array_is_list([1 => 'a'])); $sparse = ['a', 'b', 'c']; unset($sparse[1]); var_dump(array_is_list($sparse));"
        ),
        "bool(true)\nbool(true)\nbool(false)\nbool(false)\n"
    );
}

#[test]
fn array_cursor_functions_track_reset_end_next_prev_current_and_key() {
    assert_eq!(
        run_php(
            "<?php $values = ['first' => 1, 'middle' => 2, 'last' => 3]; echo reset($values), ':', key($values), '|'; echo next($values), ':', key($values), ':', current($values), '|'; echo end($values), ':', key($values), '|'; echo prev($values), ':', key($values), '|'; $empty = []; var_dump(reset($empty));"
        ),
        "1:first|2:middle:2|3:last|2:middle|bool(false)\n"
    );
}

#[test]
fn array_cursor_survives_unset_and_reappend_across_storage_transition() {
    assert_eq!(
        run_php(
            "<?php $array = ['first']; reset($array); for ($expected = 0; $expected <= 10; $expected++) { $key = key($array); echo $key, ','; next($array); unset($array[$key]); $array[] = 'next'; } echo '|'; $array = ['a', 'b', 'c']; next($array); unset($array[0]); echo key($array), current($array), ','; unset($array[1]); echo key($array), current($array);"
        ),
        "0,1,2,3,4,5,6,7,8,9,10,|1b,2c"
    );
}

// === array_reverse ===

#[test]
fn test_e2e_array_reverse() {
    assert_eq!(
        run_php("<?php $a = array_reverse([1, 2, 3]); echo $a[0]; echo $a[1]; echo $a[2];"),
        "321"
    );
}

// === array_merge ===

#[test]
fn test_e2e_array_merge() {
    assert_eq!(
        run_php(
            "<?php $a = array_merge([1, 2], [3, 4]); echo $a[0]; echo $a[1]; echo $a[2]; echo $a[3];"
        ),
        "1234"
    );
}

// === Type functions ===

#[test]
fn test_e2e_intval() {
    assert_eq!(run_php("<?php echo intval('42abc');"), "42");
}

#[test]
fn test_e2e_intval_float() {
    assert_eq!(run_php("<?php echo intval(3);"), "3");
}

#[test]
fn test_e2e_strval() {
    assert_eq!(run_php("<?php echo strval(42);"), "42");
}

#[test]
fn test_e2e_is_array_true() {
    assert_eq!(
        run_php("<?php echo is_array([1, 2]) ? 'yes' : 'no';"),
        "yes"
    );
}

#[test]
fn test_e2e_is_array_false() {
    assert_eq!(run_php("<?php echo is_array(42) ? 'yes' : 'no';"), "no");
}

#[test]
fn test_e2e_is_string() {
    assert_eq!(
        run_php("<?php echo is_string('hello') ? 'yes' : 'no';"),
        "yes"
    );
}

#[test]
fn test_e2e_is_int() {
    assert_eq!(run_php("<?php echo is_int(42) ? 'yes' : 'no';"), "yes");
}

#[test]
fn test_e2e_is_null_check() {
    assert_eq!(run_php("<?php echo is_null(null) ? 'yes' : 'no';"), "yes");
}

#[test]
fn test_e2e_is_numeric_int() {
    assert_eq!(run_php("<?php echo is_numeric(42) ? 'yes' : 'no';"), "yes");
}

#[test]
fn test_e2e_is_numeric_string() {
    assert_eq!(
        run_php("<?php echo is_numeric('3.14') ? 'yes' : 'no';"),
        "yes"
    );
}

#[test]
fn test_e2e_is_numeric_false() {
    assert_eq!(
        run_php("<?php echo is_numeric('hello') ? 'yes' : 'no';"),
        "no"
    );
}

#[test]
fn test_e2e_gettype() {
    assert_eq!(run_php("<?php echo gettype(42);"), "integer");
    assert_eq!(run_php("<?php echo gettype('hi');"), "string");
    assert_eq!(run_php("<?php echo gettype(null);"), "NULL");
    assert_eq!(run_php("<?php echo gettype(true);"), "boolean");
    assert_eq!(run_php("<?php echo gettype([1]);"), "array");
}

// === Math functions ===

#[test]
fn test_e2e_abs() {
    assert_eq!(run_php("<?php echo abs(-5);"), "5");
    assert_eq!(run_php("<?php echo abs(3);"), "3");
}

#[test]
fn test_e2e_max_min() {
    assert_eq!(run_php("<?php echo max(3, 7);"), "7");
    assert_eq!(run_php("<?php echo min(3, 7);"), "3");
}

#[test]
fn test_e2e_pow() {
    assert_eq!(run_php("<?php echo pow(2, 10);"), "1024");
}

// === var_dump ===

#[test]
fn test_e2e_var_dump_int() {
    assert_eq!(run_php("<?php var_dump(42);"), "int(42)\n");
}

#[test]
fn test_e2e_var_dump_string() {
    assert_eq!(run_php("<?php var_dump('hello');"), "string(5) \"hello\"\n");
}

#[test]
fn test_e2e_var_dump_array() {
    assert_eq!(
        run_php("<?php var_dump([1, 2]);"),
        "array(2) {\n  [0]=>\n  int(1)\n  [1]=>\n  int(2)\n}\n"
    );
}

#[test]
fn test_e2e_var_dump_null() {
    assert_eq!(run_php("<?php var_dump(null);"), "NULL\n");
}

#[test]
fn test_e2e_var_dump_bool() {
    assert_eq!(run_php("<?php var_dump(true);"), "bool(true)\n");
    assert_eq!(run_php("<?php var_dump(false);"), "bool(false)\n");
}

#[test]
fn var_dump_objects_expose_visibility_uninitialized_and_dynamic_properties() {
    assert_eq!(
        run_php(
            r#"<?php
class DumpParent {
    private $same = 'parent';
    protected $guard = 'protected';
    public $open = 'public';
    public int $typed;
    public $removed = 'gone';
}
#[AllowDynamicProperties]
class DumpChild extends DumpParent {
    private $same = 'child';
    public $nullable;
}
$value = new DumpChild();
unset($value->removed);
$value->dynamic = 'dynamic';
var_dump($value);
"#,
        ),
        "object(DumpChild)#1 (6) {\n  [\"same\":\"DumpParent\":private]=>\n  string(6) \"parent\"\n  [\"guard\":protected]=>\n  string(9) \"protected\"\n  [\"open\"]=>\n  string(6) \"public\"\n  [\"typed\"]=>\n  uninitialized(int)\n  [\"same\":\"DumpChild\":private]=>\n  string(5) \"child\"\n  [\"nullable\"]=>\n  NULL\n  [\"dynamic\"]=>\n  string(7) \"dynamic\"\n}\n"
    );
}

#[test]
fn var_dump_tracks_shared_and_recycled_request_local_object_handles() {
    assert_eq!(
        run_php(
            r#"<?php
$first = new stdClass;
$alias = $first;
var_dump($first, $alias);
$first = new stdClass;
var_dump($first);
$first = new stdClass;
var_dump($first);
unset($first, $alias);
$reused = new stdClass;
var_dump($reused);
"#,
        ),
        concat!(
            "object(stdClass)#1 (0) {\n}\n",
            "object(stdClass)#1 (0) {\n}\n",
            "object(stdClass)#2 (0) {\n}\n",
            "object(stdClass)#3 (0) {\n}\n",
            "object(stdClass)#1 (0) {\n}\n",
        )
    );
}

#[test]
fn spl_object_hash_formats_the_php_85_object_handle_and_tracks_live_identity() {
    assert_eq!(
        run_php(
            r#"<?php
$first = new stdClass;
$second = new stdClass;
$closure = static fn() => null;
echo spl_object_id($first), ':', spl_object_hash($first), "\n";
echo spl_object_id($second), ':', spl_object_hash($second), "\n";
var_dump(spl_object_hash($first) === spl_object_hash($first));
var_dump(spl_object_hash($first) !== spl_object_hash($second));
echo spl_object_id($closure), ':', spl_object_hash($closure), "\n";
$clone = clone $first;
$cloneHash = spl_object_hash($clone);
echo spl_object_id($clone), ':', $cloneHash, "\n";
var_dump($cloneHash !== spl_object_hash($first));
unset($clone);

foreach ([null, true, []] as $invalid) {
    try { spl_object_hash($invalid); }
    catch (TypeError $error) { echo $error->getMessage(), "\n"; }
    try { spl_object_id($invalid); }
    catch (TypeError $error) { echo $error->getMessage(), "\n"; }
}
"#,
        ),
        concat!(
            "1:00000000000000010000000000000000\n",
            "2:00000000000000020000000000000000\n",
            "bool(true)\n",
            "bool(true)\n",
            "3:00000000000000030000000000000000\n",
            "4:00000000000000040000000000000000\n",
            "bool(true)\n",
            "spl_object_hash(): Argument #1 ($object) must be of type object, null given\n",
            "spl_object_id(): Argument #1 ($object) must be of type object, null given\n",
            "spl_object_hash(): Argument #1 ($object) must be of type object, true given\n",
            "spl_object_id(): Argument #1 ($object) must be of type object, true given\n",
            "spl_object_hash(): Argument #1 ($object) must be of type object, array given\n",
            "spl_object_id(): Argument #1 ($object) must be of type object, array given\n",
        )
    );
}

// === print_r ===

#[test]
fn test_e2e_print_r_array() {
    assert_eq!(
        run_php("<?php print_r([1, 2, 3]);"),
        "Array\n(\n    [0] => 1\n    [1] => 2\n    [2] => 3\n)\n"
    );
}

#[test]
fn print_r_return_mode_returns_without_writing_output() {
    assert_eq!(
        run_php("<?php $result = print_r(['a' => 1, 2], true); var_dump($result);"),
        "string(36) \"Array\n(\n    [a] => 1\n    [0] => 2\n)\n\"\n"
    );
}

#[test]
fn enum_print_r_and_var_export_preserve_case_identity() {
    assert_eq!(
        run_php(
            r#"<?php
namespace Domain;
enum State: string { case Ready = "ready"; }
print_r(State::Ready);
echo var_export(State::Ready, true), "\n";
echo var_export([State::Ready], true), "\n";
"#
        ),
        "Domain\\State Enum:string\n(\n    [name] => Ready\n    [value] => ready\n)\n\\Domain\\State::Ready\narray (\n  0 =>\n  \\Domain\\State::Ready,\n)\n"
    );
}

// === Practical combinations ===

#[test]
fn test_e2e_foreach_count_loop() {
    assert_eq!(
        run_php(
            "<?php $a = [10, 20, 30]; $n = count($a); for ($i = 0; $i < $n; $i++) { echo $a[$i]; }"
        ),
        "102030"
    );
}

#[test]
fn test_e2e_explode_foreach() {
    assert_eq!(
        run_php("<?php $csv = 'a,b,c'; foreach (explode(',', $csv) as $v) { echo $v; }"),
        "abc"
    );
}

#[test]
fn test_e2e_string_processing_pipeline() {
    assert_eq!(
        run_php(
            "<?php $s = '  Hello World  '; $s = trim($s); $s = strtolower($s); $s = str_replace(' ', '_', $s); echo $s;"
        ),
        "hello_world"
    );
}

#[test]
fn test_e2e_array_filter_manual() {
    assert_eq!(
        run_php(
            "<?php $a = [1, 2, 3, 4, 5, 6]; $even = []; foreach ($a as $v) { if ($v % 2 == 0) { $even[] = $v; } } echo implode(',', $even);"
        ),
        "2,4,6"
    );
}

#[test]
fn test_e2e_word_count() {
    assert_eq!(
        run_php(
            "<?php $s = 'the quick brown fox jumps over the lazy dog'; $words = explode(' ', $s); echo count($words);"
        ),
        "9"
    );
}

#[test]
fn test_e2e_gmdate_formats_http_date_in_utc() {
    assert_eq!(
        run_php("<?php echo gmdate('D, d M Y H:i:s T', 0);"),
        "Thu, 01 Jan 1970 00:00:00 GMT"
    );
}
#[test]
fn test_error_handler_api_is_available_for_warning_guards() {
    assert_eq!(
        run_php(
            "<?php function handleWarning($type, $message) { return true; } var_dump(set_error_handler('handleWarning')); var_dump(restore_error_handler());"
        ),
        "NULL\nbool(true)\n"
    );
}

#[test]
fn trigger_error_routes_allowed_levels_through_the_registered_handler() {
    assert_eq!(
        run_php_with_source_context(
            "<?php function handleUserError($level, $message, $file, $line) { echo $level, ':', $message, ':', strlen($file), ':', $line; return true; } set_error_handler('handleUserError', E_USER_WARNING); var_dump(trigger_error('careful', E_USER_WARNING)); var_dump(user_error('quiet', E_USER_NOTICE));",
            "/virtual/core.php",
            "/virtual",
        ),
        concat!(
            "512:careful:17:1bool(true)\n",
            "\nNotice: quiet in /virtual/core.php on line 1\n",
            "bool(true)\n",
        )
    );
}

#[test]
fn detached_callbacks_preserve_reference_capture_identity() {
    assert_eq!(
        run_php(
            r#"<?php
$state = 'a';
set_error_handler(function(int $level, string $message) use (&$state) {
    $state .= 'b';
});
trigger_error('handled');
echo $state, '|';
array_map(function(string $value) use (&$state) {
    $state .= $value;
}, ['c']);
echo $state;
"#,
        ),
        "ab|abc"
    );
}

#[test]
fn trigger_error_emits_unhandled_php_82_user_diagnostics_at_the_callsite() {
    assert_eq!(
        run_php_with_source_context(
            "<?php\ntrigger_error('notice');\ntrigger_error('warning', E_USER_WARNING);\ntrigger_error('old', E_USER_DEPRECATED);",
            "/virtual/core.php",
            "/virtual",
        ),
        concat!(
            "\nNotice: notice in /virtual/core.php on line 2\n",
            "\nWarning: warning in /virtual/core.php on line 3\n",
            "\nDeprecated: old in /virtual/core.php on line 4\n",
        )
    );
}

#[test]
fn trigger_error_rejects_non_user_error_levels() {
    assert_eq!(
        run_php(
            "<?php try { trigger_error('bad', E_WARNING); } catch (ValueError $error) { echo get_class($error); }"
        ),
        "ValueError"
    );
}

#[test]
fn is_iterable_accepts_arrays_and_traversable_objects_only() {
    assert_eq!(
        run_php(
            "<?php $storage = new SplObjectStorage(); echo is_iterable([]) ? 'array' : 'bad'; echo ':'; echo is_iterable($storage) ? 'object' : 'bad'; echo ':'; echo is_iterable(new stdClass()) ? 'bad' : 'plain'; echo ':'; echo is_iterable('text') ? 'bad' : 'scalar';"
        ),
        "array:object:plain:scalar"
    );
}

#[test]
fn json_unescaped_constants_and_flags_are_available() {
    assert_eq!(
        run_php(
            "<?php echo JSON_UNESCAPED_SLASHES, ':', JSON_UNESCAPED_UNICODE, '|'; echo json_encode(['path' => '/a/b', 'word' => 'Příliš'], JSON_UNESCAPED_SLASHES | JSON_UNESCAPED_UNICODE);"
        ),
        "64:256|{\"path\":\"/a/b\",\"word\":\"Příliš\"}"
    );
}

#[test]
fn gc_mem_caches_reports_no_zend_allocator_cache_and_supports_namespace_fallback() {
    assert_eq!(
        run_php("<?php namespace Fixture; var_dump(gc_mem_caches());"),
        "int(0)\n"
    );
}

#[test]
fn gc_controls_expose_request_local_state_through_ini_get() {
    assert_eq!(
        run_php(
            "<?php var_dump(gc_enabled()); gc_disable(); var_dump(gc_enabled()); echo ini_get('zend.enable_gc'); gc_enable(); var_dump(gc_enabled()); echo ini_get('zend.enable_gc');"
        ),
        "bool(true)\nbool(false)\n0bool(true)\n1"
    );
}

#[test]
fn ini_set_returns_previous_values_and_mutates_the_admitted_request_state() {
    assert_eq!(
        run_php(
            r#"<?php
var_dump(ini_set('unknown.option', 'value'));
var_dump(ini_set('zend.enable_gc', '0'), gc_enabled(), ini_get('zend.enable_gc'));
var_dump(ini_set('zend.enable_gc', 'on'), gc_enabled(), ini_get('zend.enable_gc'));
var_dump(ini_set('zend.exception_string_param_max_len', '-1'));
var_dump(ini_set('zend.exception_string_param_max_len', '1000000'));
var_dump(ini_set('zend.exception_string_param_max_len', '20'));
echo ini_get('zend.exception_string_param_max_len');
"#,
        ),
        concat!(
            "bool(false)\n",
            "string(1) \"1\"\nbool(false)\nstring(1) \"0\"\n",
            "string(1) \"0\"\nbool(true)\nstring(2) \"on\"\n",
            "bool(false)\n",
            "string(2) \"15\"\n",
            "string(7) \"1000000\"\n",
            "20"
        )
    );
}

#[test]
fn parse_ini_supports_typed_sections_raw_bytes_and_integer_expressions() {
    assert_eq!(
        run_php(
            r#"<?php
$typed = parse_ini_string("[service]\nport=8080\nenabled=yes\nmissing=null\n", true, INI_SCANNER_TYPED);
var_dump($typed['service']['port'], $typed['service']['enabled'], $typed['service']['missing']);
$raw = parse_ini_string("token=\"a;b\" ; ignored\nmask=(1|2)&3", false, INI_SCANNER_RAW);
echo $raw['token'], ':', $raw['mask'], '|';
$normal = parse_ini_string("mask=(1|2)&3\nfull=E_ALL", false, INI_SCANNER_NORMAL);
echo $normal['mask'], ':', $normal['full'];
"#
        ),
        "int(8080)\nbool(true)\nNULL\na;b:(1|2)&3|3:30719"
    );
}

#[test]
fn ini_parse_quantity_matches_php_bases_multipliers_warnings_and_call_boundary() {
    assert_eq!(
        run_php(
            r#"<?php
set_error_handler(function ($level, $message) {
    echo $level, ':', $message, "\n";
});

foreach (['0x0b', '-0Xbeef', '0b101', '0o77', '077', '2K', '3m', '4 G'] as $quantity) {
    var_dump(ini_parse_quantity($quantity));
}
foreach (['0x+0', '0b2', '08', '1.5K', '123 abc'] as $quantity) {
    var_dump(ini_parse_quantity($quantity));
}
var_dump(ini_parse_quantity(12), ini_parse_quantity(false), ini_parse_quantity(null));

class QuantityText {
    public function __toString(): string { return '0x10K'; }
}
var_dump(ini_parse_quantity(new QuantityText()));

foreach ([[], new stdClass(), STDIN, function () {}] as $value) {
    try {
        ini_parse_quantity($value);
    } catch (TypeError $error) {
        echo $error->getMessage(), "\n";
    }
}

class BrokenQuantityText {
    public function __toString(): string { throw new Exception('string stop'); }
}
try {
    ini_parse_quantity(new BrokenQuantityText());
} catch (Exception $error) {
    echo $error->getMessage(), "\n";
}

set_error_handler(function ($level, $message) {
    throw new Exception("handled:$level:$message");
});
try {
    ini_parse_quantity('0x+0');
} catch (Exception $error) {
    echo $error->getMessage(), "\n";
}
"#
        ),
        concat!(
            "int(11)\nint(-48879)\nint(5)\nint(63)\nint(63)\n",
            "int(2048)\nint(3145728)\nint(4294967296)\n",
            "2:Invalid quantity \"0x+0\": no digits after base prefix, interpreting as \"0\" for backwards compatibility\nint(0)\n",
            "2:Invalid quantity \"0b2\": no valid leading digits, interpreting as \"0\" for backwards compatibility\nint(0)\n",
            "2:Invalid quantity \"08\": unknown multiplier \"8\", interpreting as \"0\" for backwards compatibility\nint(0)\n",
            "2:Invalid quantity \"1.5K\", interpreting as \"1K\" for backwards compatibility\nint(1024)\n",
            "2:Invalid quantity \"123 abc\": unknown multiplier \"c\", interpreting as \"123 \" for backwards compatibility\nint(123)\n",
            "8192:ini_parse_quantity(): Passing null to parameter #1 ($shorthand) of type string is deprecated\n",
            "int(12)\nint(0)\nint(0)\nint(16384)\n",
            "ini_parse_quantity(): Argument #1 ($shorthand) must be of type string, array given\n",
            "ini_parse_quantity(): Argument #1 ($shorthand) must be of type string, stdClass given\n",
            "ini_parse_quantity(): Argument #1 ($shorthand) must be of type string, resource given\n",
            "ini_parse_quantity(): Argument #1 ($shorthand) must be of type string, Closure given\n",
            "string stop\n",
            "handled:2:Invalid quantity \"0x+0\": no digits after base prefix, interpreting as \"0\" for backwards compatibility\n"
        )
    );

    assert_eq!(
        run_php(
            r#"<?php
declare(strict_types=1);
foreach ([false, true, 12, 1.5] as $value) {
    try {
        ini_parse_quantity($value);
    } catch (TypeError $error) {
        echo $error->getMessage(), "\n";
    }
}
var_dump(ini_parse_quantity('1K'));
"#
        ),
        concat!(
            "ini_parse_quantity(): Argument #1 ($shorthand) must be of type string, false given\n",
            "ini_parse_quantity(): Argument #1 ($shorthand) must be of type string, true given\n",
            "ini_parse_quantity(): Argument #1 ($shorthand) must be of type string, int given\n",
            "ini_parse_quantity(): Argument #1 ($shorthand) must be of type string, float given\n",
            "int(1024)\n"
        )
    );
}

#[test]
fn base_convert_matches_php_bases_precision_diagnostics_and_call_boundary() {
    assert_eq!(
        run_php(
            r#"<?php
set_error_handler(function ($level, $message) {
    echo $level, ':', $message, "\n";
    return true;
});

foreach ([['a37334', 16, 2], ["\t0Xff\n", 16, 10], ['0b101', 2, 10], ['0o77', 8, 10], ['zz', 36, 10]] as $case) {
    var_dump(base_convert($case[0], $case[1], $case[2]));
}
foreach ([['-10', 10, 2], ['&4#2', 10, 10], ['12304560', 2, 10]] as $case) {
    var_dump(base_convert($case[0], $case[1], $case[2]));
}
var_dump(base_convert('9223372036854775807', 10, 16));
var_dump(base_convert('9223372036854775808', 10, 10));
var_dump(base_convert('ffffffffffffffff', 16, 10));

class BaseText {
    public function __toString(): string { return '0Xff'; }
}
var_dump(base_convert(null, 10, 2));
var_dump(base_convert(10.9, 10, 2));
var_dump(base_convert(new BaseText(), 16, 10));
var_dump(base_convert('10', 2.9, 10));
var_dump(base_convert('10', 10, '2.9'));

foreach ([["10", 1, 10], ["10", 10, 37]] as $case) {
    try {
        base_convert($case[0], $case[1], $case[2]);
    } catch (ValueError $error) {
        echo $error->getMessage(), "\n";
    }
}
try {
    base_convert([], 10, 2);
} catch (TypeError $error) {
    echo $error->getMessage(), "\n";
}
class BaseCallTarget {}
try {
    basecalltarget::missing();
} catch (Error $error) {
    echo $error->getMessage(), "\n";
}

set_error_handler(function ($level, $message) {
    throw new Exception("handled:$level:$message");
});
try {
    base_convert('-10', 10, 2);
} catch (Exception $error) {
    echo $error->getMessage(), "\n";
}
restore_error_handler();
try {
    base_convert(str_repeat('1', 2000), 2, 10);
} catch (ValueError $error) {
    echo $error->getMessage(), "\n";
}
"#
        ),
        concat!(
            "string(24) \"101000110111001100110100\"\n",
            "string(3) \"255\"\nstring(1) \"5\"\nstring(2) \"63\"\nstring(4) \"1295\"\n",
            "8192:Invalid characters passed for attempted conversion, these have been ignored\nstring(4) \"1010\"\n",
            "8192:Invalid characters passed for attempted conversion, these have been ignored\nstring(2) \"42\"\n",
            "8192:Invalid characters passed for attempted conversion, these have been ignored\nstring(1) \"4\"\n",
            "string(16) \"7fffffffffffffff\"\n",
            "string(19) \"9223372036854776028\"\n",
            "string(20) \"18446744073709552046\"\n",
            "8192:base_convert(): Passing null to parameter #1 ($num) of type string is deprecated\nstring(1) \"0\"\n",
            "8192:Invalid characters passed for attempted conversion, these have been ignored\nstring(7) \"1101101\"\n",
            "string(3) \"255\"\n",
            "8192:Implicit conversion from float 2.9 to int loses precision\nstring(1) \"2\"\n",
            "8192:Implicit conversion from float-string \"2.9\" to int loses precision\nstring(4) \"1010\"\n",
            "base_convert(): Argument #2 ($from_base) must be between 2 and 36 (inclusive)\n",
            "base_convert(): Argument #3 ($to_base) must be between 2 and 36 (inclusive)\n",
            "base_convert(): Argument #1 ($num) must be of type string, array given\n",
            "Call to undefined method BaseCallTarget::missing()\n",
            "handled:8192:Invalid characters passed for attempted conversion, these have been ignored\n",
            "An infinite value cannot be converted to base 10\n"
        )
    );

    assert_eq!(
        run_php(
            r#"<?php
declare(strict_types=1);
foreach ([false, true, 12, 1.5] as $value) {
    try {
        base_convert($value, 10, 2);
    } catch (TypeError $error) {
        echo $error->getMessage(), "\n";
    }
}
foreach ([2.0, '2'] as $value) {
    try {
        base_convert('10', $value, 10);
    } catch (TypeError $error) {
        echo $error->getMessage(), "\n";
    }
}
var_dump(base_convert('10', 10, 2));
"#
        ),
        concat!(
            "base_convert(): Argument #1 ($num) must be of type string, false given\n",
            "base_convert(): Argument #1 ($num) must be of type string, true given\n",
            "base_convert(): Argument #1 ($num) must be of type string, int given\n",
            "base_convert(): Argument #1 ($num) must be of type string, float given\n",
            "base_convert(): Argument #2 ($from_base) must be of type int, float given\n",
            "base_convert(): Argument #2 ($from_base) must be of type int, string given\n",
            "string(4) \"1010\"\n"
        )
    );
}

#[test]
fn get_defined_functions_reports_real_inventory_and_php_85_argument_diagnostics() {
    assert_eq!(
        run_php(
            r#"<?php
function AlphaInventory() {}
function MixedInventory() {}
class InventoryMethods {
    public function hiddenFromFunctionList() {}
}

$functions = get_defined_functions();
echo implode(',', array_keys($functions)), '|';
echo in_array('strlen', $functions['internal'], true) ? 'strlen' : 'missing';
echo ':', in_array('base_convert', $functions['internal'], true) ? 'base' : 'missing';
echo '|', implode(',', $functions['user']), "\n";

set_error_handler(function ($level, $message) {
    echo $level, ':', $message, "\n";
    return true;
});
var_dump(count(get_defined_functions(true)) === 2);
var_dump(count(get_defined_functions(false)) === 2);
var_dump(count(get_defined_functions(null)) === 2);
var_dump(count(get_defined_functions(NAN)) === 2);
try {
    get_defined_functions([]);
} catch (TypeError $error) {
    echo $error->getMessage(), "\n";
}

set_error_handler(function ($level, $message) {
    throw new Exception("handled:$level:$message");
});
try {
    get_defined_functions(true);
} catch (Exception $error) {
    echo $error->getMessage(), "\n";
}
"#
        ),
        concat!(
            "internal,user|strlen:base|alphainventory,mixedinventory\n",
            "8192:get_defined_functions(): The $exclude_disabled parameter has no effect since PHP 8.0\n",
            "bool(true)\n",
            "8192:get_defined_functions(): The $exclude_disabled parameter has no effect since PHP 8.0\n",
            "bool(true)\n",
            "8192:get_defined_functions(): Passing null to parameter #1 ($exclude_disabled) of type bool is deprecated\n",
            "8192:get_defined_functions(): The $exclude_disabled parameter has no effect since PHP 8.0\n",
            "bool(true)\n",
            "2:unexpected NAN value was coerced to bool\n",
            "8192:get_defined_functions(): The $exclude_disabled parameter has no effect since PHP 8.0\n",
            "bool(true)\n",
            "get_defined_functions(): Argument #1 ($exclude_disabled) must be of type bool, array given\n",
            "handled:8192:get_defined_functions(): The $exclude_disabled parameter has no effect since PHP 8.0\n"
        )
    );

    assert_eq!(
        run_php(
            r#"<?php
declare(strict_types=1);
set_error_handler(function ($level, $message) {
    echo $level, ':', $message, "\n";
    return true;
});
get_defined_functions(false);
foreach ([null, 0, '0'] as $value) {
    try {
        get_defined_functions($value);
    } catch (TypeError $error) {
        echo $error->getMessage(), "\n";
    }
}
"#
        ),
        concat!(
            "8192:get_defined_functions(): The $exclude_disabled parameter has no effect since PHP 8.0\n",
            "get_defined_functions(): Argument #1 ($exclude_disabled) must be of type bool, null given\n",
            "get_defined_functions(): Argument #1 ($exclude_disabled) must be of type bool, int given\n",
            "get_defined_functions(): Argument #1 ($exclude_disabled) must be of type bool, string given\n"
        )
    );
}

#[test]
fn extract_updates_the_caller_scope_with_flags_references_and_atomic_errors() {
    assert_eq!(
        run_php(
            r#"<?php
function probeExtractScope() {
    $existing = 'old';
    $input = ['a' => 1, 'existing' => 'new', 7 => 3];
    echo extract($input, EXTR_SKIP), ":$a:$existing|";
    echo extract($input, EXTR_PREFIX_INVALID, 'p'), ":$p_7|";

    $refs = ['x' => 1];
    extract($refs, EXTR_REFS);
    $x = 9;
    echo $refs['x'], '|';

    try {
        extract(['this' => 42, 'late' => 24]);
    } catch (Error $error) {
        echo $error->getMessage(), ':', isset($late) ? 'partial' : 'atomic', '|';
    }

    $dynamic = 'extract';
    try {
        $dynamic(['z' => 1]);
    } catch (Error $error) {
        echo $error->getMessage(), '|';
    }

    $name = 'runtime';
    $$name = 5;
    $vars = get_defined_vars();
    echo $vars['existing'], ':', $vars['runtime'];
}
probeExtractScope();
"#
        ),
        "1:1:old|3:3|9|Cannot re-assign $this:atomic|Cannot call extract() dynamically|new:5"
    );
}

#[test]
fn pathinfo_supports_component_flags_used_by_source_loaders() {
    assert_eq!(
        run_php(
            "<?php echo PATHINFO_ALL, ':'; echo pathinfo('/a/archive.tar.gz', PATHINFO_DIRNAME), ':'; echo pathinfo('/a/archive.tar.gz', PATHINFO_BASENAME), ':'; echo pathinfo('/a/archive.tar.gz', PATHINFO_EXTENSION), ':'; echo pathinfo('/a/archive.tar.gz', PATHINFO_FILENAME);"
        ),
        "15:/a:archive.tar.gz:gz:archive.tar"
    );
}
