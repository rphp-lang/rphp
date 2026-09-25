mod common;

use common::run_php;

#[test]
fn readonly_increment_errors_keep_the_failed_operation_as_previous() {
    assert_eq!(
        run_php(
            r#"<?php
class RestrictedPendingOperand { function __construct(public readonly mixed $value) {} }
function showPendingOperand($error) {
    do { echo get_class($error), ':', $error->getMessage(), '|'; }
    while ($error = $error->getPrevious());
}
$object = new RestrictedPendingOperand(new stdClass);
try { $object->value++; } catch (Throwable $e) { showPendingOperand($e); }
try { ++$object->value; } catch (Throwable $e) { showPendingOperand($e); }
try { $object->value--; } catch (Throwable $e) { showPendingOperand($e); }
try { --$object->value; } catch (Throwable $e) { showPendingOperand($e); }
$object = new RestrictedPendingOperand([]);
try { $object->value++; } catch (Throwable $e) { showPendingOperand($e); }
try { --$object->value; } catch (Throwable $e) { showPendingOperand($e); }
"#
        ),
        concat!(
            "Error:Cannot modify readonly property RestrictedPendingOperand::$value|TypeError:Cannot increment stdClass|",
            "Error:Cannot modify readonly property RestrictedPendingOperand::$value|TypeError:Cannot increment stdClass|",
            "Error:Cannot modify readonly property RestrictedPendingOperand::$value|TypeError:Cannot decrement stdClass|",
            "Error:Cannot modify readonly property RestrictedPendingOperand::$value|TypeError:Cannot decrement stdClass|",
            "Error:Cannot modify readonly property RestrictedPendingOperand::$value|TypeError:Cannot increment array|",
            "Error:Cannot modify readonly property RestrictedPendingOperand::$value|TypeError:Cannot decrement array|"
        )
    );
}

#[test]
fn a_throwing_increment_diagnostic_precedes_readonly_validation_without_mutation() {
    assert_eq!(
        run_php(
            r#"<?php
class DiagnosticRestrictedOperand { function __construct(public readonly mixed $value) {} }
set_error_handler(function ($level, $message) { echo 'handler|'; throw new LogicException('diagnostic'); });
foreach (['word', false] as $value) {
    $object = new DiagnosticRestrictedOperand($value);
    try { ++$object->value; }
    catch (Throwable $error) {
        do { echo get_class($error), ':', $error->getMessage(), '|'; }
        while ($error = $error->getPrevious());
    }
    echo (int)($object->value === $value), '|';
}
"#
        ),
        concat!(
            "handler|Error:Cannot modify readonly property DiagnosticRestrictedOperand::$value|LogicException:diagnostic|1|",
            "handler|Error:Cannot modify readonly property DiagnosticRestrictedOperand::$value|LogicException:diagnostic|1|"
        )
    );
}

#[test]
fn asymmetric_set_visibility_is_checked_after_the_value_operation() {
    assert_eq!(
        run_php(
            r#"<?php
class PrivatePendingOperand {
    public private(set) mixed $value;
    function __construct($value) { $this->value = $value; }
}
set_error_handler(function ($level, $message) { echo 'handler|'; return true; });
foreach ([new stdClass, [], false] as $value) {
    $object = new PrivatePendingOperand($value);
    try { ++$object->value; }
    catch (Throwable $error) {
        do { echo get_class($error), ':', $error->getMessage(), '|'; }
        while ($error = $error->getPrevious());
    }
    echo (int)($object->value === $value), '|';
}
"#
        ),
        concat!(
            "Error:Cannot modify private(set) property PrivatePendingOperand::$value from global scope|TypeError:Cannot increment stdClass|1|",
            "Error:Cannot modify private(set) property PrivatePendingOperand::$value from global scope|TypeError:Cannot increment array|1|",
            "handler|Error:Cannot modify private(set) property PrivatePendingOperand::$value from global scope|1|"
        )
    );
}

#[test]
fn failed_overloaded_increment_materializes_storage_without_entering_a_handler() {
    assert_eq!(
        run_php(
            r#"<?php
class CopiedPendingOperand { function __get($name) { echo 'get-copy|'; return new stdClass; } }
class AliasedPendingOperand {
    public $stored;
    function __construct() { $this->stored = new stdClass; }
    function &__get($name) { echo 'get-ref|'; return $this->stored; }
}
set_error_handler(function ($level, $message) { echo 'handler|'; return true; });
foreach ([new CopiedPendingOperand, new AliasedPendingOperand] as $object) {
    try { $object->missing++; } catch (Throwable $error) { echo get_class($error), '|'; }
    echo 'stored:', (int)property_exists($object, 'missing'), '|';
}
echo (int)($object->missing === $object->stored), '|';
"#
        ),
        "get-copy|TypeError|stored:1|get-ref|TypeError|stored:1|1|"
    );
}

#[test]
fn pending_operation_errors_skip_magic_and_hook_setters() {
    assert_eq!(
        run_php(
            r#"<?php
class MagicPendingSetter {
    function __get($name) { echo 'magic-get|'; return new stdClass; }
    function __set($name, $value) { echo 'magic-set|'; }
}
class HookPendingSetter {
    public mixed $value {
        get { echo 'hook-get|'; return new stdClass; }
        set { echo 'hook-set|'; }
    }
}
foreach ([new MagicPendingSetter, new HookPendingSetter] as $object) {
    try { ++$object->value; }
    catch (Throwable $error) { echo get_class($error), ':', $error->getMessage(), '|'; }
}
"#
        ),
        "magic-get|TypeError:Cannot increment stdClass|hook-get|TypeError:Cannot increment stdClass|"
    );
}

#[test]
fn a_replacing_destructor_observes_completed_failed_property_storage() {
    assert_eq!(
        run_php(
            r#"<?php
class FailingPendingReceiver {
    public $stored;
    function __construct() { $this->stored = new stdClass; }
    function &__get($name) { return $this->stored; }
    function __destruct() {
        echo 'stored:', (int)property_exists($this, 'missing'), '|';
        throw new LogicException('retired');
    }
}
function createFailingPendingReceiver() { return new FailingPendingReceiver; }
set_error_handler(function ($level, $message) { echo 'handler|'; return true; });
try { ++createFailingPendingReceiver()->missing; }
catch (Throwable $error) {
    do { echo get_class($error), ':', $error->getMessage(), '|'; }
    while ($error = $error->getPrevious());
}
"#
        ),
        "stored:1|LogicException:retired|TypeError:Cannot increment stdClass|"
    );
}

#[test]
fn failed_increment_keeps_default_deprecations_but_honors_suppression() {
    assert_eq!(
        run_php(
            r#"<?php
class DeprecatedPendingOperand { function __get($name) { return new stdClass; } }
foreach ([false, true] as $suppressed) {
    $object = new DeprecatedPendingOperand;
    ob_start();
    try { if ($suppressed) @$object->value++; else $object->value++; }
    catch (Throwable $error) { echo 'caught|'; }
    $output = ob_get_clean();
    echo 'deprecation:', (int)str_contains($output, 'Creation of dynamic property DeprecatedPendingOperand::$value is deprecated'), '|';
    echo 'caught:', (int)str_contains($output, 'caught|'), '|';
    echo 'stored:', (int)property_exists($object, 'value'), '|';
}
"#
        ),
        "deprecation:1|caught:1|stored:1|deprecation:0|caught:1|stored:1|"
    );
}

#[test]
fn throwing_diagnostics_do_not_commit_a_successful_increment_value() {
    assert_eq!(
        run_php(
            r#"<?php
class DiagnosticWriteOperand { function __construct(public $value) {} }
set_error_handler(function () { throw new LogicException('diagnostic'); });
foreach (['az', false, null] as $value) {
    $object = new DiagnosticWriteOperand($value);
    try { $object->value++; } catch (Throwable $e) { echo get_class($e), '|'; }
    var_dump($object->value);
}
"#
        ),
        "LogicException|string(2) \"az\"\nLogicException|bool(false)\nint(1)\n"
    );
}
