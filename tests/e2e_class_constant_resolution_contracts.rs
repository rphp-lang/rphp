mod common;
use common::*;

#[test]
fn nested_dynamic_class_constant_errors_keep_the_use_site_origin() {
    assert_eq!(
        run_php_with_source_context(
            r#"<?php
function probe() {
    try { y::{5}::y; } catch (Error $error) {
        echo basename($error->getFile()), ':', $error->getLine(), ':', $error->getMessage();
    }
}
probe();
"#,
            "/virtual/class-constant-origin.php",
            "/virtual",
        ),
        "class-constant-origin.php:3:Class \"y\" not found"
    );
}

#[test]
fn dynamic_owner_validation_precedes_the_constant_name_expression() {
    assert_eq!(
        run_php(
            r#"<?php
function owner() { echo 'owner|'; return []; }
function constant_name() { echo 'name|'; return 'VALUE'; }
try {
    owner()::{constant_name()};
} catch (Error $error) {
    echo $error->getMessage();
}
"#,
        ),
        "owner|Class name must be a valid object or a string"
    );
}

#[test]
fn inherited_constant_diagnostics_materialize_once_and_global_magic_names_are_empty() {
    assert_eq!(
        run_php(
            r#"<?php
set_error_handler(function($level, $message) {
    echo "$level:$message|";
    return true;
});
class BaseDiagnostic { const VALUE = 5 % 1.5; }
class ChildDiagnostic extends BaseDiagnostic {}
echo BaseDiagnostic::VALUE, ':', ChildDiagnostic::VALUE, '|';
const METHOD_NAME = __METHOD__;
const FUNCTION_NAME = __FUNCTION__;
var_dump(METHOD_NAME, FUNCTION_NAME);

trait ParentScopeProbe {
    function parentName() {
        try { return parent::class; }
        catch (Error $error) { echo $error->getMessage(); }
    }
}
class RootWithoutParent { use ParentScopeProbe; }
(new RootWithoutParent())->parentName();
"#,
        ),
        concat!(
            "8192:Implicit conversion from float 1.5 to int loses precision|0:0|",
            "string(0) \"\"\n",
            "string(0) \"\"\n",
            "Cannot use \"parent\" when current class scope has no parent",
        )
    );
}
