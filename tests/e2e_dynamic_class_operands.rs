mod common;
use common::*;

#[test]
fn dynamic_class_operands_preserve_objects_references_and_type_errors() {
    assert_eq!(
        run_php(
            r#"<?php
class OperandTarget {}
function inspect($label, $value) {
    echo $label, ':';
    try { var_dump($value()); }
    catch (Throwable $error) { echo $error::class, ':', $error->getMessage(), "\n"; }
}
$object = new OperandTarget;
$reference =& $object;
$null = null;
$int = 2;
$string = OperandTarget::class;
inspect('object', fn() => $object::class);
inspect('reference', fn() => $reference::class);
inspect('temporary', fn() => (new OperandTarget)::class);
inspect('null', fn() => $null::class);
inspect('int', fn() => $int::class);
inspect('string', fn() => $string::class);
var_dump($object === $reference);
"#,
        ),
        concat!(
            "object:string(13) \"OperandTarget\"\n",
            "reference:string(13) \"OperandTarget\"\n",
            "temporary:string(13) \"OperandTarget\"\n",
            "null:TypeError:Cannot use \"::class\" on null\n",
            "int:TypeError:Cannot use \"::class\" on int\n",
            "string:TypeError:Cannot use \"::class\" on string\n",
            "bool(true)\n",
        )
    );
}

#[test]
fn invalid_dynamic_static_access_is_catchable_without_operand_mutation() {
    assert_eq!(
        run_php(
            r#"<?php
class StaticOperandTarget {
    public const TOKEN = 'constant';
    public static string $slot = 'property';
}
function source($label, $value) { echo "source:$label|"; return $value; }
foreach ([null, 2] as $value) {
    try { source('constant', $value)::TOKEN; }
    catch (Throwable $error) { echo $error::class, ':', $error->getMessage(), "\n"; }
    try { source('property', $value)::$slot; }
    catch (Throwable $error) { echo $error::class, ':', $error->getMessage(), "\n"; }
}
$class = StaticOperandTarget::class;
$object = new StaticOperandTarget;
echo $class::TOKEN, ':', $object::TOKEN, ':', $class::$slot, ':', $object::$slot;
"#,
        ),
        concat!(
            "source:constant|Error:Class name must be a valid object or a string\n",
            "source:property|Error:Class name must be a valid object or a string\n",
            "source:constant|Error:Class name must be a valid object or a string\n",
            "source:property|Error:Class name must be a valid object or a string\n",
            "constant:constant:property:property",
        )
    );
}

#[test]
fn dynamic_array_callbacks_validate_shape_before_members() {
    assert_eq!(
        run_php(
            r#"<?php
class CallbackOperandTarget { public static function ping() { echo 'called'; } }
foreach ([[1 => CallbackOperandTarget::class, 2 => 'ping'], [0, 'ping'], [CallbackOperandTarget::class, 0]] as $callback) {
    try { $callback(); }
    catch (Throwable $error) { echo $error->getMessage(), "\n"; }
}
[CallbackOperandTarget::class, 'ping']();
$reversed = [1 => 'ping', 0 => CallbackOperandTarget::class];
$reversed();
"#,
        ),
        concat!(
            "Array callback has to contain indices 0 and 1\n",
            "Class name must be a valid object or a string\n",
            "Method name must be a string\n",
            "calledcalled",
        )
    );
}

#[test]
fn literal_and_constant_expression_class_owners_fail_during_compilation() {
    for source in [
        "<?php (-0)::$slot;",
        "<?php (2)::class;",
        "<?php []::TOKEN::TOKEN;",
    ] {
        assert!(
            run_php_expect_error(source)
                .to_string()
                .contains("Illegal class name")
        );
    }
    assert!(
        run_php_expect_error("<?php const INVALID_OWNER = [0]::class;")
            .to_string()
            .contains("(expression)::class cannot be used in constant expressions")
    );
}
