mod common;

use common::{run_php, run_php_expect_error_with_source_context};

#[test]
fn dynamic_variables_share_statically_named_slots_in_global_and_function_scopes() {
    let output = run_php(
        r#"<?php
$name = 'value';
$$name = 41;
echo $value, ':', ${$name}, "\n";

function local_scope($name) {
    $$name = 'local';
    echo $value, ':', $$name, "\n";
}

$value = 'global';
local_scope('value');
echo $value, "\n";
"#,
    );

    assert_eq!(output, "41:41\nlocal:local\nglobal\n");
}

#[test]
fn runtime_only_names_are_frame_local_and_support_isset_and_unset() {
    let output = run_php(
        r#"<?php
function probe($name) {
    $$name = 42;
    var_dump(isset($$name));
    echo ${$name}, "\n";
    unset($$name);
    var_dump(isset($$name));
}

probe('created_at_runtime');
var_dump(isset($created_at_runtime));
"#,
    );

    assert_eq!(output, "bool(true)\n42\nbool(false)\nbool(false)\n");
}

#[test]
fn indirect_callable_postfix_binds_after_variable_resolution() {
    let output = run_php(
        r#"<?php
function decorate($value) { return '[' . $value . ']'; }
$callable = 'decorate';
$selector = 'callable';
echo $$selector('ok'), "\n";
"#,
    );

    assert_eq!(output, "[ok]\n");
}

#[test]
fn dynamic_reference_bindings_preserve_one_reference_cell() {
    let output = run_php(
        r#"<?php
$name = 'slot';
$source = 3;
$$name =& $source;
$source = 4;
$alias =& $$name;
$alias = 5;
echo $source, ':', $slot, "\n";
"#,
    );

    assert_eq!(output, "5:5\n");
}

#[test]
fn dynamic_array_append_reference_preserves_self_reference() {
    let output = run_php(
        r#"<?php
$array = [1];
$name = 'array';
$$name[] =& $$name;
$$name[0] = 2;
var_dump($array);
"#,
    );

    assert_eq!(
        output,
        "array(2) {\n  [0]=>\n  int(2)\n  [1]=>\n  *RECURSION*\n}\n"
    );
}

#[test]
fn dynamic_global_binding_targets_the_request_symbol_table() {
    let output = run_php(
        r#"<?php
$shared = 40;
function increment_dynamic_global($selector) {
    global $$selector;
    $$selector += 2;
}
increment_dynamic_global('shared');
echo $shared, "\n";
"#,
    );

    assert_eq!(output, "42\n");
}

#[test]
fn dynamic_coalesce_and_array_writeback_preserve_values() {
    let output = run_php(
        r#"<?php
$name = 'slot';
$slot = 7;
echo ($$name ??= 11), ':', $slot, "\n";
unset($slot);
echo ($$name ??= 11), ':', $slot, "\n";

$array_name = 'items';
$items = ['count' => 2];
$$array_name['count'] += 3;
$$array_name['count']++;
echo $items['count'], "\n";
"#,
    );

    assert_eq!(output, "7:7\n11:11\n6\n");
}

#[test]
fn indirect_object_and_static_members_follow_php_indirection_depth() {
    let output = run_php(
        r#"<?php
class DynamicMemberFixture {
    public static $b = 'static-b';
    public static $p = 'static-p';
    public static $q = 'static-q';
    public $b = 'object-b';
    public $p = 'object-p';
    public $q = 'object-q';

    public static function p() { echo "static-method-p\n"; }
    public static function q() { echo "static-method-q\n"; }
    public function objectP() { echo "object-method-p\n"; }
    public function objectQ() { echo "object-method-q\n"; }
}

$b = 'p';
$p = 'q';
echo DynamicMemberFixture::$b, ':', DynamicMemberFixture::$$b, "\n";
DynamicMemberFixture::$b();
DynamicMemberFixture::$$b();

$class = 'DynamicMemberFixture';
$object = new DynamicMemberFixture;
echo $class::$b, ':', $class::$$b, ':', $object::$b, "\n";

$method = 'objectP';
$objectP = 'objectQ';
echo $object->$b, ':', $object->$$b, "\n";
$object->$method();
$object->$$method();
"#,
    );

    assert_eq!(
        output,
        "static-b:static-p\nstatic-method-p\nstatic-method-q\nstatic-b:static-p:static-b\nobject-p:object-q\nobject-method-p\nobject-method-q\n"
    );
}

#[test]
fn destructuring_can_assign_through_a_runtime_variable_name() {
    let output = run_php(
        r#"<?php
function destructure_dynamic() {
    $target = 'first';
    list($$target, $second) = [10, 20];
    echo $first, ':', $second, "\n";
}
destructure_dynamic();
"#,
    );

    assert_eq!(output, "10:20\n");
}

#[test]
fn string_increment_can_feed_nested_dynamic_names() {
    let output = run_php(
        r#"<?php
$selector = 'b';
$c = 'callable_name';
$callable_name = 'strtolower';
echo ${${++$selector}}('MIXED'), "\n";

$suffix = 'z';
$numeric = '09';
++ $suffix;
++ $numeric;
echo $suffix, ':', $numeric, "\n";
"#,
    );

    assert_eq!(
        output,
        "\nDeprecated: Increment on non-numeric string is deprecated, use str_increment() instead in <main> on line 5\nmixed\n\nDeprecated: Increment on non-numeric string is deprecated, use str_increment() instead in <main> on line 9\naa:10\n"
    );
}

#[test]
fn object_names_use_to_string_and_propagate_conversion_errors() {
    let output = run_php(
        r#"<?php
class RuntimeName {
    public function __toString() { return 'selected'; }
}
class ThrowingRuntimeName {
    public function __toString() { throw new Exception('name conversion'); }
}

$selected = 42;
$name = new RuntimeName;
echo ${$name}, "\n";

try {
    isset(${new ThrowingRuntimeName});
} catch (Exception $exception) {
    echo $exception->getMessage(), "\n";
}

try {
    global ${new stdClass};
} catch (Error $error) {
    echo $error->getMessage(), "\n";
}
"#,
    );

    assert_eq!(
        output,
        "42\nname conversion\nObject of class stdClass could not be converted to string\n"
    );
}

#[test]
fn indirect_this_reads_the_receiver_but_cannot_rebind_it() {
    let output = run_php(
        r#"<?php
class IndirectThisFixture {
    public function probe() {
        $name = 'this';
        var_dump($$name === $this);
        try {
            $$name = null;
        } catch (Error $error) {
            echo $error->getMessage(), "\n";
        }
        try {
            $alias =& $$name;
        } catch (Error $error) {
            echo $error->getMessage(), "\n";
        }
    }
}

(new IndirectThisFixture)->probe();
"#,
    );

    assert_eq!(
        output,
        "bool(true)\nCannot re-assign $this\nCannot re-assign $this\n"
    );
}

#[test]
fn literal_this_write_targets_fail_during_compilation_with_the_target_line() {
    for statement in [
        "$this = replacement();",
        "$this = isset(replacement());",
        "$this =& $replacement;",
        "$this ??= replacement();",
        "$this ??= isset(replacement());",
        "foreach ($values as $this) {}",
        "foreach ($values as $this => $value) {}",
        "foreach ($values as &$this) {}",
        "foreach ($values as list($this)) {}",
        "foreach ($values as [&$this]) {}",
        "try {} catch (Exception $this) {}",
    ] {
        let source = format!(
            "<?php\nclass Subject {{\n    public function unreachable() {{\n        {statement}\n    }}\n}}"
        );
        let error = run_php_expect_error_with_source_context(
            &source,
            "/virtual/this-write-target.php",
            "/virtual",
        );

        assert_eq!(
            format!("{error:?}"),
            "Fatal(\"Cannot re-assign $this in /virtual/this-write-target.php on line 4\")",
            "unexpected diagnostic for {statement}"
        );
    }
}

#[test]
fn dynamic_variable_mutations_retain_one_converted_name_across_reentry() {
    let output = run_php(
        r#"<?php
set_error_handler(function($errno, $message) {
    global $name;
    echo $message, "\n";
    $name = 'other';
});

$name = 'post';
$$name++;
var_dump($name, $post, $other ?? null);

$name = 'pre';
++$$name;
var_dump($name, $pre, $other ?? null);

$name = 'compound';
$$name += 2;
var_dump($name, $compound, $other ?? null);

class RuntimeMutationName {
    public function __toString() { echo "convert\n"; return 'target'; }
}
$key = new RuntimeMutationName;
$target = 1;
${$key}++;
var_dump($target);
"#,
    );

    assert_eq!(
        output,
        concat!(
            "Undefined variable $post\n",
            "string(5) \"other\"\n",
            "int(1)\n",
            "NULL\n",
            "Undefined variable $pre\n",
            "string(5) \"other\"\n",
            "int(1)\n",
            "NULL\n",
            "Undefined variable $compound\n",
            "string(5) \"other\"\n",
            "int(2)\n",
            "NULL\n",
            "convert\n",
            "int(2)\n",
        )
    );
}
