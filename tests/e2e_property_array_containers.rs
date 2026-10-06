mod common;

use common::run_php;

#[test]
fn writable_property_arrays_keep_php_copy_and_alias_identity() {
    assert_eq!(
        run_php(
            r#"<?php
class Box { public array $items = [1]; }
$o = new Box();
$copy = $o->items;
$clone = clone $o;
for ($i = 0; $i < 2; $i++) { $o->items[] = $i + 2; }
$projection = get_object_vars($o);
var_dump(ReflectionReference::fromArrayElement($projection, 'items') === null);
$clone2 = clone $o;
$o->items[0] = 7;
echo json_encode([$copy, $clone->items, $clone2->items, $o->items]), "\n";
$alias =& $o->items;
$o->items[] = 8;
$alias[] = 9;
echo json_encode($o->items), "\n";
foreach ($o->items as &$v) { $v += 10; }
unset($v);
echo json_encode($alias), "\n";
unset($alias);
try { $o->items = 'bad'; } catch (TypeError $e) { echo 'typed\n'; }
echo json_encode($o->items), "\n";
"#
        ),
        r#"bool(true)
[[1],[1],[1,2,3],[7,2,3]]
[7,2,3,8,9]
[17,12,13,18,19]
typed\n[17,12,13,18,19]
"#
    );
}

#[test]
fn property_array_diagnostics_preserve_reentrant_replacement() {
    assert_eq!(
        run_php(
            r#"<?php
class Container { public array $items = [0 => 10]; }
foreach (['replace', 'unset', 'append'] as $mode) {
    $o = new Container();
    set_error_handler(function ($level, $message) use ($o, $mode) {
        echo $mode, ':before:', json_encode($o->items), "\n";
        if ($mode === 'replace') { $o->items = ['handler' => 20]; }
        elseif ($mode === 'unset') { unset($o->items); }
        else { $o->items[] = 20; }
        return true;
    });
    $o->items[1.5] = 30;
    restore_error_handler();
    echo $mode, ':after:', json_encode(get_object_vars($o)), "\n";
}
class ObserveRelease {
    public function __construct(public object $owner) {}
    public function __destruct() { echo 'drop:', json_encode($this->owner->items), "\n"; }
}
$o = new Container();
$o->items[0] = new ObserveRelease($o);
$o->items[0] = 40;
echo 'done:', json_encode($o->items), "\n";
"#
        ),
        r#"replace:before:[10]
replace:after:{"items":{"handler":20}}
unset:before:[10]
unset:after:[]
append:before:[10]
append:after:{"items":[10,20]}
drop:[40]
done:[40]
"#
    );
}

#[test]
fn property_array_diagnostics_preserve_published_snapshots() {
    assert_eq!(
        run_php(
            r#"<?php
class ReentryBox { public array $items = [10]; }
foreach ([false, true] as $aliased) {
    foreach (['passive', 'copy', 'clone', 'append', 'replace', 'unset', 'throw'] as $mode) {
        $o = new ReentryBox();
        $saved = null;
        if ($aliased) { $alias =& $o->items; }
        set_error_handler(function ($level, $message) use ($o, $mode, &$saved) {
            if ($mode === 'copy') { $saved = $o->items; }
            elseif ($mode === 'clone') { $saved = clone $o; }
            elseif ($mode === 'append') { $o->items[] = 20; }
            elseif ($mode === 'replace') { $o->items = ['handler' => 20]; }
            elseif ($mode === 'unset') { unset($o->items); }
            elseif ($mode === 'throw') { $o->items[] = 20; throw new Exception('handler'); }
            return true;
        });
        try { $o->items[1.5] = 30; } catch (Exception $e) { echo 'caught:'; }
        restore_error_handler();
        echo $aliased ? 'alias:' : 'value:', $mode, ':', json_encode(get_object_vars($o)), ':', json_encode($saved), ':', $aliased ? json_encode($alias) : '-', "\n";
        unset($alias, $o, $saved);
    }
}
"#
        ),
        r#"value:passive:{"items":[10,30]}:null:-
value:copy:{"items":[10]}:[10]:-
value:clone:{"items":[10]}:{"items":[10]}:-
value:append:{"items":[10,20]}:null:-
value:replace:{"items":{"handler":20}}:null:-
value:unset:[]:null:-
caught:value:throw:{"items":[10,20]}:null:-
alias:passive:{"items":[10,30]}:null:[10,30]
alias:copy:{"items":[10]}:[10]:[10]
alias:clone:{"items":[10,30]}:{"items":[10,30]}:[10,30]
alias:append:{"items":[10,20]}:null:[10,20]
alias:replace:{"items":{"handler":20}}:null:{"handler":20}
alias:unset:[]:null:[10,30]
caught:alias:throw:{"items":[10,20]}:null:[10,20]
"#
    );
}

#[test]
fn array_key_errors_retire_temporary_keys_before_catch_rebinding() {
    assert_eq!(
        run_php(
            r#"<?php
ini_set('zend.exception_ignore_args', '1');
class RejectedKey { public function __destruct() { echo 'key|'; } }
class PriorCatch { public function __destruct() { echo 'prior|'; } }
class InvalidKeyBox { public array $items = []; }
foreach ([false, true] as $property) {
    $box = new InvalidKeyBox();
    $items = [];
    $error = new PriorCatch();
    try {
        if ($property) { $box->items[new RejectedKey()] = 1; }
        else { $items[new RejectedKey()] = 1; }
    } catch (TypeError $error) { echo 'caught|'; }
    echo 'after|';
}
"#
        ),
        r#"key|prior|caught|after|key|prior|caught|after|"#
    );
}

#[test]
fn property_array_diagnostics_retire_detached_payloads() {
    assert_eq!(
        run_php(
            r#"<?php
ini_set('zend.exception_ignore_args', '1');
class PayloadBox { public array $items = []; }
class Payload {
    public function __construct(public string $name) {}
    public function __destruct() { echo 'drop:', $this->name, '|'; }
}
foreach ([false, true] as $aliased) {
    foreach (['replace', 'unset', 'copy', 'throw', 'replace-throw', 'unset-throw'] as $mode) {
        echo $aliased ? 'alias:' : 'value:', $mode, ':';
        $o = new PayloadBox();
        $o->items[] = new Payload('old');
        $saved = null;
        if ($aliased) { $alias =& $o->items; }
        set_error_handler(function ($level, $message) use ($o, $mode, &$saved) {
            echo 'enter|';
            if ($mode === 'replace' || $mode === 'replace-throw') { $o->items = []; }
            elseif ($mode === 'unset' || $mode === 'unset-throw') { unset($o->items); }
            elseif ($mode === 'copy') { $saved = $o->items; }
            echo 'leave|';
            if ($mode === 'throw' || $mode === 'replace-throw' || $mode === 'unset-throw') { throw new Exception('handler'); }
            return true;
        });
        try { $o->items[1.5] = 30; } catch (Exception $e) { echo 'caught|'; }
        restore_error_handler();
        echo 'after|';
        unset($alias, $o, $saved);
        echo "end\n";
    }
}
"#
        ),
        r#"value:replace:enter|leave|drop:old|after|end
value:unset:enter|leave|drop:old|after|end
value:copy:enter|leave|after|drop:old|end
value:throw:enter|leave|caught|after|drop:old|end
value:replace-throw:enter|leave|drop:old|caught|after|end
value:unset-throw:enter|leave|drop:old|caught|after|end
alias:replace:enter|leave|drop:old|after|end
alias:unset:enter|leave|after|drop:old|end
alias:copy:enter|leave|after|drop:old|end
alias:throw:enter|leave|caught|after|drop:old|end
alias:replace-throw:enter|leave|drop:old|caught|after|end
alias:unset-throw:enter|leave|caught|after|drop:old|end
"#
    );
}

#[test]
fn property_array_retirement_preserves_throw_resurrection_and_write_order() {
    assert_eq!(
        run_php(
            r#"<?php
ini_set('zend.exception_ignore_args', '1');
class CompletionBox { public array $items = []; }
class CompletingPayload {
    public function __construct(public int $number, public string $mode) {}
    public function __destruct() {
        echo 'drop:', $this->number, '|';
        if ($this->number === 0) {
            if ($this->mode === 'throw') { throw new Exception('destructor'); }
            if ($this->mode === 'publish') { $GLOBALS['saved'] = $this; }
            if ($this->mode === 'write') { $GLOBALS['box']->items[] = 42; }
        }
    }
}
foreach (['replace', 'unset'] as $operation) {
    foreach (['throw', 'publish', 'write'] as $mode) {
        foreach ([false, true] as $handlerThrows) {
            echo $operation, ':', $mode, ':', (int)$handlerThrows, ':';
            $box = new CompletionBox();
            $box->items[] = new CompletingPayload(0, $mode);
            $box->items[] = new CompletingPayload(1, $mode);
            $saved = null;
            set_error_handler(function () use ($box, $operation, $handlerThrows) {
                echo 'enter|';
                if ($operation === 'replace') { $box->items = []; }
                else { unset($box->items); }
                echo 'leave|';
                if ($handlerThrows) { throw new Exception('handler'); }
                return true;
            });
            try { $box->items[1.5] = 30; }
            catch (Exception $e) {
                echo 'caught:', $e->getMessage(), ':', $e->getPrevious()?->getMessage() ?? '-', '|';
            }
            restore_error_handler();
            echo 'after:', json_encode(get_object_vars($box)), ':', $saved === null ? '-' : 'saved', '|';
            unset($box, $saved, $e);
            echo "end\n";
        }
    }
}
"#
        ),
        r#"replace:throw:0:enter|leave|drop:0|drop:1|caught:destructor:-|after:{"items":[]}:-|end
replace:throw:1:enter|leave|drop:0|drop:1|caught:destructor:handler|after:{"items":[]}:-|end
replace:publish:0:enter|leave|drop:0|drop:1|after:{"items":[]}:saved|end
replace:publish:1:enter|leave|drop:0|drop:1|caught:handler:-|after:{"items":[]}:saved|end
replace:write:0:enter|leave|drop:0|drop:1|after:{"items":[42]}:-|end
replace:write:1:enter|leave|drop:0|drop:1|caught:handler:-|after:{"items":[42]}:-|end
unset:throw:0:enter|leave|drop:0|drop:1|caught:destructor:-|after:[]:-|end
unset:throw:1:enter|leave|drop:0|drop:1|caught:destructor:handler|after:[]:-|end
unset:publish:0:enter|leave|drop:0|drop:1|after:[]:saved|end
unset:publish:1:enter|leave|drop:0|drop:1|caught:handler:-|after:[]:saved|end
unset:write:0:enter|leave|drop:0|drop:1|after:{"items":[42]}:-|end
unset:write:1:enter|leave|drop:0|drop:1|caught:handler:-|after:{"items":[42]}:-|end
"#
    );
}

#[test]
fn property_array_unset_preserves_diagnostic_owner_completion() {
    assert_eq!(
        run_php(
            r#"<?php
ini_set('zend.exception_ignore_args', '1');
class CompletionBox { public array $items = []; }
class CompletingPayload {
    public function __construct(public int $number, public string $mode) {}
    public function __destruct() {
        echo 'drop:', $this->number, '|';
        if ($this->number === 0) {
            if ($this->mode === 'throw') { throw new Exception('destructor'); }
            if ($this->mode === 'publish') { $GLOBALS['saved'] = $this; }
            if ($this->mode === 'write') { $GLOBALS['box']->items[] = 42; }
        }
    }
}
foreach (['replace', 'unset'] as $operation) {
    foreach (['throw', 'publish', 'write'] as $mode) {
        foreach ([false, true] as $handlerThrows) {
            echo $operation, ':', $mode, ':', (int)$handlerThrows, ':';
            $box = new CompletionBox();
            $box->items[] = new CompletingPayload(0, $mode);
            $box->items[] = new CompletingPayload(1, $mode);
            $saved = null;
            set_error_handler(function () use ($box, $operation, $handlerThrows) {
                echo 'enter|';
                if ($operation === 'replace') { $box->items = []; }
                else { unset($box->items); }
                echo 'leave|';
                if ($handlerThrows) { throw new Exception('handler'); }
                return true;
            });
            try { unset($box->items[1.5]); }
            catch (Exception $e) {
                echo 'caught:', $e->getMessage(), ':', $e->getPrevious()?->getMessage() ?? '-', '|';
            }
            restore_error_handler();
            echo 'after:', json_encode(get_object_vars($box)), ':', $saved === null ? '-' : 'saved', '|';
            unset($box, $saved, $e);
            echo "end\n";
        }
    }
}
"#
        ),
        r#"replace:throw:0:enter|leave|drop:0|drop:1|caught:destructor:-|after:{"items":[]}:-|end
replace:throw:1:enter|leave|drop:0|drop:1|caught:destructor:handler|after:{"items":[]}:-|end
replace:publish:0:enter|leave|drop:0|drop:1|after:{"items":[]}:saved|end
replace:publish:1:enter|leave|drop:0|drop:1|caught:handler:-|after:{"items":[]}:saved|end
replace:write:0:enter|leave|drop:0|drop:1|after:{"items":[42]}:-|end
replace:write:1:enter|leave|drop:0|drop:1|caught:handler:-|after:{"items":[42]}:-|end
unset:throw:0:enter|leave|drop:0|drop:1|caught:destructor:-|after:[]:-|end
unset:throw:1:enter|leave|drop:0|drop:1|caught:destructor:handler|after:[]:-|end
unset:publish:0:enter|leave|drop:0|drop:1|after:[]:saved|end
unset:publish:1:enter|leave|drop:0|drop:1|caught:handler:-|after:[]:saved|end
unset:write:0:enter|leave|drop:0|drop:1|after:{"items":[42]}:-|end
unset:write:1:enter|leave|drop:0|drop:1|caught:handler:-|after:{"items":[42]}:-|end
"#
    );
}
