mod common;

use common::{run_php, run_php_expect_error};

#[test]
fn assertion_source_preserves_control_flow_dynamic_static_access_and_interpolation() {
    assert_eq!(
        run_php(
            r#"<?php
try {
    assert(false && (function () {
        declare(ALPHA=1,BETA=2);
        if ($left) {} elseif ($right) {} else;
        $call = Handler::${$name . "_handler"}();
        $property = ${$owner . "_class"}::$slot;
        $text = "$labels[1] {$object->value}";
        $legacy = "${label} {$labels[1]}";
        namespace\probe();
    })());
} catch (AssertionError $error) { echo $error->getMessage(), "\n"; }
"#,
        ),
        concat!(
            "assert(false && (function () {\n",
            "    declare(ALPHA = 1, BETA = 2);\n",
            "    if ($left) {\n",
            "    } elseif ($right) {\n",
            "    } else {\n",
            "    }\n",
            "    $call = Handler::${$name . '_handler'}();\n",
            "    $property = ${$owner . '_class'}::$slot;\n",
            "    $text = \"{$labels[1]} {$object->value}\";\n",
            "    $legacy = \"{$label} {$labels[1]}\";\n",
            "    namespace\\probe();\n",
            "})())\n",
        )
    );
}

#[test]
fn clone_and_backtick_source_capture_does_not_run_short_circuited_operands() {
    assert_eq!(
        run_php(
            r#"<?php
$hits = 0;
function mark(array $value): array { global $hits; ++$hits; return $value; }
$object = new stdClass();
foreach ([
    fn() => assert(false && ($copy = clone($object, ['one' => mark([])]))),
    fn() => assert(false && ($copy = clone $object)),
    fn() => assert(false && ($output = `printf unreachable`)),
] as $probe) {
    try { $probe(); } catch (AssertionError $error) { echo $error->getMessage(), "\n"; }
}
echo "hits=$hits\n";
try { `printf reached`; }
catch (Error $error) { echo get_class($error), ':', $error->getMessage(), "\n"; }
"#,
        ),
        concat!(
            "assert(false && ($copy = \\clone($object, ['one' => mark([])])))\n",
            "assert(false && ($copy = \\clone($object)))\n",
            "assert(false && ($output = `printf unreachable`))\n",
            "hits=0\n",
            "Error:Backtick shell execution is not supported\n",
        )
    );
}

#[test]
fn assertion_source_preserves_anonymous_class_attributes_and_promoted_hooks() {
    assert_eq!(
        run_php(
            r#"<?php
try {
    assert(false && new #[Container('demo'), Secondary] class {
        public function __construct(
            #[Input(1)] public private(set) final bool $ready = false {
                final set => $this->ready = $value;
            }
        ) {}
    });
} catch (AssertionError $error) { echo $error->getMessage(), "\n"; }
"#,
        ),
        concat!(
            "assert(false && new #[Container('demo'), Secondary] class {\n",
            "    public function __construct(#[Input(1)] public private(set) final bool $ready = false {\n",
            "        final set => $this->ready = $value;\n",
            "    }) {\n",
            "    }\n",
            "\n",
            "})\n",
        )
    );
}

#[test]
fn only_a_constant_false_assertion_elides_nested_semantic_diagnostics() {
    let error = run_php_expect_error(
        r#"<?php
$condition = true;
assert($condition && (function ($value) use ($value) {}));
"#,
    );
    assert_eq!(
        format!("{error:?}"),
        "Fatal(\"Cannot use lexical variable $value as a parameter name on line 3\")"
    );
}

#[test]
fn simple_interpolated_assertion_source_is_reconstructed_from_the_expression_tree() {
    assert_eq!(
        run_php(
            r#"<?php
$value = 'rphp';
try { assert("left $value" === "right $value"); }
catch (AssertionError $error) { echo $error->getMessage(), "\n"; }
try { assert(assertion: false); }
catch (AssertionError $error) { echo $error->getMessage(), "\n"; }
"#,
        ),
        concat!(
            "assert(\"left $value\" === \"right $value\")\n",
            "assert(assertion: false)\n",
        )
    );
}

#[test]
fn escaped_and_braced_interpolation_keeps_lossless_assertion_source() {
    assert_eq!(
        run_php(
            r#"<?php
$value = 'rphp';
try { assert("\$literal \\n $value" === "other {$value}"); }
catch (AssertionError $error) { echo $error->getMessage(), "\n"; }
try { assert("{$value}tail" === "never"); }
catch (AssertionError $error) { echo $error->getMessage(), "\n"; }
try { assert("{$value}[]" === "never"); }
catch (AssertionError $error) { echo $error->getMessage(), "\n"; }
try { assert(false && "${value[0]}[]" === "never"); }
catch (AssertionError $error) { echo $error->getMessage(), "\n"; }
try { assert(false && " ${'---'} " === "never"); }
catch (AssertionError $error) { echo $error->getMessage(), "\n"; }
"#,
        ),
        concat!(
            "assert(\"\\$literal \\\\n $value\" === \"other $value\")\n",
            "assert(\"{$value}tail\" === 'never')\n",
            "assert(\"{$value}[]\" === 'never')\n",
            "assert(false && \"{$value[0]}[]\" === 'never')\n",
            "assert(false && \" ${---} \" === 'never')\n",
        )
    );
}

#[test]
fn inactive_synthesized_assert_returns_true_without_evaluating_its_argument() {
    assert_eq!(
        run_php(
            r#"<?php
assert_options(ASSERT_ACTIVE, 0);
$hits = 0;
$result = assert(++$hits && "left $hits" === "right $hits");
var_dump($result, $hits);
"#,
        ),
        concat!(
            "\nDeprecated: Function assert_options() is deprecated since 8.3 in <main> on line 2\n",
            "bool(true)\n",
            "int(0)\n",
        )
    );
}
