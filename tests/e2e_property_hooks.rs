mod common;

use common::{run_php, run_php_expect_error};

#[test]
fn getter_hook_runs_and_getter_only_property_rejects_writes() {
    assert_eq!(
        run_php(
            r#"<?php
class Reading {
    public $answer {
        get { return 6 * 7; }
    }
}
$reading = new Reading();
var_dump($reading->answer);
try { $reading->answer = 9; } catch (Error $error) { echo $error->getMessage(), "\n"; }
"#,
        ),
        "int(42)\nProperty Reading::$answer is read-only\n"
    );
}

#[test]
fn getter_hook_reentrance_reads_backing_storage() {
    assert_eq!(
        run_php(
            r#"<?php
class Counter {
    public $value = 40 {
        get {
            echo __FUNCTION__, " ", __METHOD__, "\n";
            return $this->value + 2;
        }
    }
}
$counter = new Counter();
$counter->value = 40;
var_dump($counter->value);
"#,
        ),
        "$value::get Counter::$value::get\nint(42)\n"
    );
}

#[test]
fn getter_hook_inheritance_uses_method_variance_contract() {
    let error = run_php_expect_error(
        r#"<?php
class ParentReading { public int $value { get { return 1; } } }
class ChildReading extends ParentReading { public int|float $value { get { return 1; } } }
"#,
    );
    assert_eq!(
        format!("{error:?}"),
        "Fatal(\"Declaration of ChildReading::$value::get(): int|float must be compatible with ParentReading::$value::get(): int\")"
    );
}

#[test]
fn getter_hook_is_not_exposed_as_an_ordinary_method() {
    assert_eq!(
        run_php(
            r#"<?php
class SecretHook {
    public $value { get { return 42; } }
    public function visible() {}
}
var_dump(get_class_methods(SecretHook::class));
"#,
        ),
        "array(1) {\n  [0]=>\n  string(7) \"visible\"\n}\n"
    );
}
