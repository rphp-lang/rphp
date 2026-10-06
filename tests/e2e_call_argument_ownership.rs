mod common;
use common::run_php;

#[test]
fn value_arguments_survive_replacement_of_the_callers_reference() {
    assert_eq!(
        run_php(
            r#"<?php
class OwnedArgumentSnapshot { public $value = 'old'; public function __destruct() { echo 'drop|'; } }
function readOwnedArgument($value, $replace) { $replace(); echo $value->value, '|'; }
$source = new OwnedArgumentSnapshot;
$alias =& $source;
readOwnedArgument($source, function () use (&$source) { $source = null; });
echo 'after';
"#
        ),
        "old|drop|after"
    );
}

#[test]
fn already_sent_arguments_survive_later_argument_evaluation() {
    assert_eq!(
        run_php(
            r#"<?php
class OwnedPendingArgument { public $value = 'old'; public function __destruct() { echo 'drop|'; } }
function readPendingArgument($value, $unused) { echo $value->value, '|'; }
function replacePendingArgument(&$source) { $source = null; return 0; }
$source = new OwnedPendingArgument;
$alias =& $source;
readPendingArgument($source, replacePendingArgument($source));
echo 'after';
"#
        ),
        "old|drop|after"
    );
}

#[test]
fn method_receivers_survive_callback_and_later_argument_replacement() {
    assert_eq!(
        run_php(
            r#"<?php
class OwnedReceiverSnapshot {
    public $value = 'old';
    public function __destruct() { echo 'drop|'; }
    public function callback($replace) { $replace(); echo $this->value, '|'; }
    public function later($unused) { echo $this->value, '|'; }
}
function replacePendingReceiver(&$source) { $source = null; return 0; }
$source = new OwnedReceiverSnapshot;
$alias =& $source;
$source->callback(function () use (&$source) { $source = null; });
echo 'after|';
$source = new OwnedReceiverSnapshot;
$source->later(replacePendingReceiver($source));
echo 'after';
"#
        ),
        "old|drop|after|old|drop|after"
    );
}

#[test]
fn array_argument_snapshots_preserve_copy_on_write() {
    assert_eq!(
        run_php(
            r#"<?php
function readOwnedArray($value, $replace) { $replace(); echo $value['key'], '|'; }
$source = ['key' => 'old'];
$alias =& $source;
readOwnedArray($source, function () use (&$source) { $source['key'] = 'new'; });
echo $source['key'];
"#
        ),
        "old|new"
    );
}

#[test]
fn abandoned_argument_owners_run_destructors_before_catch_selection() {
    assert_eq!(
        run_php(
            r#"<?php
class PendingOwnedArgument { function __destruct() { echo 'drop|'; } }
function pendingOwnedTarget($value, $unused) { echo 'entered|'; }
function pendingOwnedFailure(&$source) { $source = null; throw new Exception('caught'); }
$source = new PendingOwnedArgument; $alias =& $source;
try { pendingOwnedTarget($source, pendingOwnedFailure($source)); } catch (Exception $e) { echo $e->getMessage(), '|'; }
echo 'after';
"#
        ),
        "drop|caught|after"
    );
}

#[test]
fn abandoned_argument_destructor_can_replace_the_original_exception() {
    assert_eq!(
        run_php(
            r#"<?php
class PendingOwnedThrow { function __destruct() { echo 'drop|'; throw new RuntimeException('replacement'); } }
function pendingOwnedThrowTarget($value, $unused) { echo 'entered|'; }
function pendingOwnedThrowFailure(&$source) { $source = null; throw new Exception('original'); }
$source = new PendingOwnedThrow; $alias =& $source;
try { pendingOwnedThrowTarget($source, pendingOwnedThrowFailure($source)); } catch (Exception $e) { echo $e->getMessage(), '|'; }
echo 'after';
"#
        ),
        "drop|replacement|after"
    );
}

#[test]
fn diagnostic_failure_preserves_destructor_exception_and_previous_chain() {
    let source = r#"<?php
class DiagnosticArgumentOwner {
    function __destruct() { echo 'drop|'; throw new RuntimeException('replacement'); }
}
#[OWNER_ATTRIBUTE]
function diagnosticArgumentTarget($owner) { echo 'entered|'; return 0; }
$source = new DiagnosticArgumentOwner; $alias =& $source;
set_error_handler(function () use (&$source) { $source = null; throw new Exception('original'); });
try { diagnosticArgumentTarget($source); }
catch (Throwable $error) {
    echo get_class($error), ':', $error->getMessage(), '|';
    $previous = $error->getPrevious();
    echo $previous === null ? 'none' : $previous->getMessage();
}
restore_error_handler();
echo '|after';
"#;
    for attribute in ["Deprecated", "NoDiscard"] {
        assert_eq!(
            run_php(&source.replace("OWNER_ATTRIBUTE", attribute)),
            "drop|RuntimeException:replacement|original|after",
            "{attribute} must retire the pending owner's exception at the diagnostic boundary"
        );
    }
}
