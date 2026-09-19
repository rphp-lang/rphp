mod common;

use common::run_php;

#[test]
fn undefined_property_write_receivers_are_silent_until_the_write_error() {
    assert_eq!(
        run_php(
            r#"<?php
set_error_handler(function ($severity, $message) {
    echo "warning:$message\n";
    return true;
});
function rhs() { echo "rhs\n"; return 42; }

try { $root->item = rhs(); } catch (Error $error) {
    echo $error->getMessage(), "\n";
}
var_dump($root);
unset($root);

try { $root->item['key']->leaf = rhs(); } catch (Error $error) {
    echo $error->getMessage(), "\n";
}
var_dump($root);
"#,
        ),
        concat!(
            "rhs\n",
            "Attempt to assign property \"item\" on null\n",
            "warning:Undefined variable $root\n",
            "NULL\n",
            "rhs\n",
            "Attempt to modify property \"item\" on null\n",
            "warning:Undefined variable $root\n",
            "NULL\n",
        )
    );
}

#[test]
fn silent_probes_distinguish_dynamic_roots_globals_and_this_receivers() {
    assert_eq!(
        run_php(
            r#"<?php
set_error_handler(function ($severity, $message) {
    echo "warning:$message\n";
    return true;
});
$name = 'missing';
var_dump(isset($$name->item));
var_dump(isset($GLOBALS));
var_dump(isset($GLOBALS['missing']));
try { var_dump(isset($this->item)); } catch (Throwable $error) {
    echo $error->getMessage(), "\n";
}
"#,
        ),
        concat!(
            "bool(false)\n",
            "bool(true)\n",
            "bool(false)\n",
            "Using $this when not in object context\n",
        )
    );
}

#[test]
fn ordinary_global_reads_warn_while_dynamic_and_compact_reads_keep_their_labels() {
    assert_eq!(
        run_php(
            r#"<?php
set_error_handler(function ($severity, $message) {
    echo "warning:$message\n";
    return true;
});
$GLOBALS['missing'];
$name = 'missing';
$$name;
compact('missing');
"#,
        ),
        concat!(
            "warning:Undefined global variable $missing\n",
            "warning:Undefined variable $missing\n",
            "warning:compact(): Undefined variable $missing\n",
        )
    );
}

#[test]
fn unset_static_cv_detaches_only_the_current_invocation() {
    assert_eq!(
        run_php(
            r#"<?php
function next_static() {
    static $value;
    ++$value;
    echo $value, "\n";
    unset($value);
    $value = 20;
}
next_static();
next_static();
next_static();
"#,
        ),
        "1\n2\n3\n"
    );
}
