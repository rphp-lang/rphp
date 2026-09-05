mod common;

use common::run_php;

#[test]
fn constructor_access_cache_distinguishes_public_aliases_from_a_private_class() {
    assert_eq!(
        run_php(
            r#"<?php
class OpenConstruction { function __construct($value) { echo 'open:', $value, "\n"; } }
class ClosedConstruction { private function __construct($value) {} }
class_alias(OpenConstruction::class, 'ConstructionAlias');
function buildSelected($class) { return new $class(print "argument\n"); }
foreach ([OpenConstruction::class, 'openconstruction', 'ConstructionAlias', ClosedConstruction::class, OpenConstruction::class] as $class) {
    try { buildSelected($class); } catch (Error $error) { echo $error->getMessage(), "\n"; }
}
try { buildSelected('\\\\OpenConstruction'); }
catch (Error $error) { echo "invalid-name\n"; }
"#
        ),
        concat!(
            "argument\nopen:1\nargument\nopen:1\nargument\nopen:1\n",
            "Call to private ClosedConstruction::__construct() from global scope\n",
            "argument\nopen:1\ninvalid-name\n"
        )
    );
}

#[test]
fn inherited_internal_methods_keep_the_receiver_in_forwarded_closure_calls() {
    assert_eq!(
        run_php(
            r#"<?php
class ForwardedProblem extends RuntimeException {
    function __construct($message) { parent::__construct($message, 19); }
    function inspect() {
        $read = fn() => parent::getMessage() . ':' . self::getCode();
        return $read();
    }
}
echo (new ForwardedProblem('forwarded'))->inspect(), "\n";
"#
        ),
        "forwarded:19\n"
    );
}

#[test]
fn static_object_calls_never_bind_this_and_keep_the_called_class() {
    assert_eq!(
        run_php(
            r#"<?php
class ScopeBase {
    static function inspect($n) { echo (int)isset($this), ':', static::class, ':', $n, "\n"; }
}
class ScopeChild extends ScopeBase {}
$object = new ScopeChild;
$alias =& $object;
for ($n = 0; $n < 2; $n++) {
    $alias->inspect($n);
    $name = 'inspect';
    $alias->$name($n);
    call_user_func([$alias, $name], $n);
}
echo get_class($object), "\n";
class InstanceScope {
    function inspect($n) { echo (int)isset($this), ':', static::class, ':', $n, "\n"; }
}
function mixedScopeCall($receiver, $n) { $receiver->inspect($n); }
foreach ([$object, new InstanceScope, $object, new InstanceScope] as $n => $receiver) {
    mixedScopeCall($receiver, $n);
}
"#
        ),
        concat!(
            "0:ScopeChild:0\n0:ScopeChild:0\n0:ScopeChild:0\n",
            "0:ScopeChild:1\n0:ScopeChild:1\n0:ScopeChild:1\nScopeChild\n",
            "0:ScopeChild:0\n1:InstanceScope:1\n0:ScopeChild:2\n1:InstanceScope:3\n"
        )
    );
}

#[test]
fn lexical_private_method_wins_over_a_child_shadow_for_all_object_call_forms() {
    assert_eq!(
        run_php(
            r#"<?php
class PrivateOwner {
    private function secret() { echo 'owner:', get_class($this), "\n"; }
    function invoke() {
        $method = 'secret';
        $this->secret(); $this->$method();
        call_user_func([$this, $method]);
        $callback = [$this, $method]; $callback();
    }
}
class PrivateShadow extends PrivateOwner {
    public function secret() { echo "shadow\n"; }
}
$instance = new PrivateShadow;
$instance->invoke();
$instance->secret();
"#
        ),
        "owner:PrivateShadow\nowner:PrivateShadow\nowner:PrivateShadow\nowner:PrivateShadow\nshadow\n"
    );
}

#[test]
fn protected_override_prototypes_apply_to_static_calls_and_callbacks() {
    assert_eq!(
        run_php(
            r#"<?php
class FamilyRoot {
    protected static function shared() { return 'root'; }
    private static function hidden() { return 'private'; }
}
class FamilyLeft extends FamilyRoot {
    protected static function shared() { return 'left'; }
    protected static function hidden() { return 'shadow'; }
    protected static function unique() { return 'unique'; }
}
class FamilyRight extends FamilyRoot {
    static function check() {
        echo FamilyLeft::shared(), "\n";
        foreach (['shared', 'hidden', 'unique'] as $method) {
            $callback = [FamilyLeft::class, $method];
            echo $method, ':', (int)is_callable($callback), "\n";
            if (is_callable($callback)) echo call_user_func($callback), "\n";
        }
        try { FamilyLeft::hidden(); } catch (Error $e) { echo $e->getMessage(), "\n"; }
        try { FamilyLeft::unique(); } catch (Error $e) { echo $e->getMessage(), "\n"; }
    }
}
FamilyRight::check();
"#
        ),
        concat!(
            "left\nshared:1\nleft\nhidden:0\nunique:0\n",
            "Call to protected method FamilyLeft::hidden() from scope FamilyRight\n",
            "Call to protected method FamilyLeft::unique() from scope FamilyRight\n"
        )
    );
}

#[test]
fn qualified_private_callbacks_share_lexical_visibility_without_exporting_it() {
    assert_eq!(
        run_php(
            r#"<?php
class SecretFactory {
    private static function payload($n) { echo 'private:', $n, "\n"; }
    static function run() {
        foreach (['SecretFactory::payload', [self::class, 'payload']] as $callback) {
            echo (int)is_callable($callback), "\n";
            call_user_func($callback, 7);
        }
    }
}
SecretFactory::run();
echo (int)is_callable('SecretFactory::payload'), ':', (int)is_callable('SecretFactory::payload', true), "\n";
"#
        ),
        "1\nprivate:7\n1\nprivate:7\n0:1\n"
    );
}

#[test]
fn inherited_private_method_existence_distinguishes_class_and_object_queries() {
    assert_eq!(
        run_php(
            r#"<?php
class ListedParent { private static function prior() {} protected function shared() {} }
class ListedChild extends ListedParent { private function own() {} }
foreach ([ListedParent::class, ListedChild::class, new ListedChild] as $target) {
    foreach (['prior', 'shared', 'own', 'missing'] as $method) echo (int)method_exists($target, $method);
    echo "\n";
}
"#
        ),
        "1100\n0110\n1110\n"
    );
}

#[test]
fn explicit_class_instance_calls_forward_this_and_late_static_class_in_closures() {
    assert_eq!(
        run_php(
            r#"<?php
class ForwardBase {
    function identify() { echo get_class($this), ':', static::class, "\n"; }
}
class ForwardChild extends ForwardBase {
    function run() {
        ForwardBase::identify();
        $callback = fn() => ForwardBase::identify(); $callback();
        $callback = fn() => call_user_func('ForwardBase::identify'); $callback();
        $callback = static fn() => ForwardBase::identify();
        try { $callback(); } catch (Error $e) { echo $e->getMessage(), "\n"; }
    }
}
(new ForwardChild)->run();
"#
        ),
        concat!(
            "ForwardChild:ForwardChild\nForwardChild:ForwardChild\nForwardChild:ForwardChild\n",
            "Non-static method ForwardBase::identify() cannot be called statically\n"
        )
    );
}

#[test]
fn constructor_visibility_is_checked_before_arguments_and_never_grants_sibling_access() {
    assert_eq!(
        run_php(
            r#"<?php
function constructorArgument() { echo "argument\n"; return 3; }
class HiddenCtor {
    private function __construct($n) { echo "constructed\n"; }
    function __destruct() { echo "destroyed\n"; }
}
class HiddenChild extends HiddenCtor {
    static function attempt() { return new self(constructorArgument()); }
}
class ProtectedCtor { protected function __construct() {} }
class ProtectedLeft extends ProtectedCtor { protected function __construct() { echo "left\n"; } }
class ProtectedRight extends ProtectedCtor {
    static function attempt() { return new ProtectedLeft; }
}
try { HiddenChild::attempt(); } catch (Error $e) { echo $e->getMessage(), "\n"; }
try { ProtectedRight::attempt(); } catch (Error $e) { echo $e->getMessage(), "\n"; }
echo "done\n";
"#
        ),
        concat!(
            "Call to private HiddenCtor::__construct() from scope HiddenChild\n",
            "Call to protected ProtectedLeft::__construct() from scope ProtectedRight\ndone\n"
        )
    );
}

#[test]
fn explicit_reflection_invocation_keeps_its_visibility_bypass() {
    assert_eq!(
        run_php(
            r#"<?php
class ReflectedOwner {
    private function value($n) { return $n + 8; }
}
$object = new ReflectedOwner;
$method = new ReflectionMethod(ReflectedOwner::class, 'value');
var_dump($method->invoke($object, 4));
var_dump(is_callable([$object, 'value']));
"#
        ),
        "int(12)\nbool(false)\n"
    );
}

#[test]
fn implicit_variable_receivers_and_static_closure_probes_do_not_share_stack_state() {
    assert_eq!(
        run_php(
            r#"<?php
class VariableReceiver {
    function run() {
        $name = 'this';
        $read = fn() => $$name;
        $probe = static fn() => [isset($this), empty($this)];
        $invalid = static fn() => $this;
        $coalesce = static fn() => $this ?? 'absent';
        for ($n = 0; $n < 3; $n++) {
            echo (int)($read() === $this), ':';
            foreach ($probe() as $value) echo (string)$value, ':';
            try { $invalid(); } catch (Error $e) { echo $e->getMessage(); }
            try { $coalesce(); } catch (Error $e) { echo ':', $e->getMessage(); }
            echo "\n";
        }
    }
}
(new VariableReceiver)->run();
"#
        ),
        "1::1:Using $this when not in object context:Using $this when not in object context\n1::1:Using $this when not in object context:Using $this when not in object context\n1::1:Using $this when not in object context:Using $this when not in object context\n"
    );
}

#[test]
fn constructor_access_precedes_named_unpack_dynamic_and_suspended_arguments() {
    assert_eq!(
        run_php(
            r#"<?php
class SealedConstruction { private function __construct($value) {} }
function argumentList() { echo 'unexpected'; return [1]; }
function deferredConstruction() { yield new SealedConstruction(yield 'unexpected'); }
try { new SealedConstruction(value: argumentList()); } catch (Error $e) { echo $e->getMessage(), "\n"; }
try { new SealedConstruction(...argumentList()); } catch (Error $e) { echo $e->getMessage(), "\n"; }
$class = SealedConstruction::class;
try { new $class(argumentList()); } catch (Error $e) { echo $e->getMessage(), "\n"; }
try { deferredConstruction()->current(); } catch (Error $e) { echo $e->getMessage(), "\n"; }
"#
        ),
        "Call to private SealedConstruction::__construct() from global scope\nCall to private SealedConstruction::__construct() from global scope\nCall to private SealedConstruction::__construct() from global scope\nCall to private SealedConstruction::__construct() from global scope\n"
    );
}

#[test]
fn constructor_context_keeps_evaluated_dynamic_class_and_reference_identity() {
    assert_eq!(
        run_php(
            r#"<?php
class ArgumentReceiver {
    function __construct(&$value, $ignored) { $value[] = 'constructed'; }
}
class OtherReceiver { function __construct(&$value, $ignored) { echo 'wrong'; } }
function changeClass(&$name) { $name = OtherReceiver::class; return 9; }
$class = ArgumentReceiver::class;
$value = ['seed']; $copy = $value;
$object = new $class($value, changeClass($class));
echo get_class($object), ':', $class, ':', implode(',', $value), ':', implode(',', $copy), "\n";
"#
        ),
        "ArgumentReceiver:OtherReceiver:seed,constructed:seed\n"
    );
}

#[test]
fn constructor_cache_rechecks_non_public_access_after_closure_rebinding() {
    assert_eq!(
        run_php(
            r#"<?php
class ScopedConstruction {
    private function __construct() { echo 'created:'; }
}
class ForeignConstruction {}
$factory = function() { return new ScopedConstruction; };
$inside = Closure::bind($factory, null, ScopedConstruction::class);
$outside = Closure::bind($factory, null, ForeignConstruction::class);
echo get_class($inside()), "\n";
try { $outside(); } catch (Error $e) { echo $e->getMessage(), "\n"; }
echo get_class($inside()), "\n";
"#
        ),
        "created:ScopedConstruction\nCall to private ScopedConstruction::__construct() from scope ForeignConstruction\ncreated:ScopedConstruction\n"
    );
}

#[test]
fn aliases_and_case_variants_retain_the_canonical_private_declaring_owner() {
    assert_eq!(
        run_php(
            r#"<?php
class GuardOriginal { private function __construct() {} private function hidden() {} }
class GuardDerived extends GuardOriginal {}
class_alias(GuardOriginal::class, 'GuardAlias');
foreach (['guardderived', 'GuardAlias'] as $class) {
    echo (int)method_exists($class, 'hidden'), ':';
    try { new $class; } catch (Error $e) { echo $e->getMessage(), "\n"; }
}
"#
        ),
        "0:Call to private GuardOriginal::__construct() from global scope\n1:Call to private GuardOriginal::__construct() from global scope\n"
    );
}
