#[test]
fn relative_callback_keywords_preserve_lexical_and_forwarding_scope() {
    assert_eq!(
        run_php(
            r#"<?php
set_error_handler(function ($level, $message) { echo 'D:', $message, '|'; return true; });
class RelativeRoot {
    public static function mark() { echo 'R:', get_called_class(), '|'; }
}
class RelativeMiddle extends RelativeRoot {
    public static function mark() { echo 'M:', get_called_class(), '|'; }
    public static function probe() {
        foreach (['self::mark', 'parent::mark', 'static::mark'] as $callback) {
            echo is_callable($callback) ? 'Y|' : 'N|';
            call_user_func($callback);
        }
        foreach ([['self', 'mark'], ['parent', 'mark'], ['static', 'mark']] as $callback) {
            call_user_func_array($callback, []);
        }
    }
}
class RelativeLeaf extends RelativeMiddle {
    public static function mark() { echo 'L:', get_called_class(), '|'; }
}
RelativeLeaf::probe();
"#,
        ),
        concat!(
            "D:Use of \"self\" in callables is deprecated|Y|",
            "D:Use of \"self\" in callables is deprecated|M:RelativeLeaf|",
            "D:Use of \"parent\" in callables is deprecated|Y|",
            "D:Use of \"parent\" in callables is deprecated|R:RelativeLeaf|",
            "D:Use of \"static\" in callables is deprecated|Y|",
            "D:Use of \"static\" in callables is deprecated|L:RelativeLeaf|",
            "D:Use of \"self\" in callables is deprecated|M:RelativeLeaf|",
            "D:Use of \"parent\" in callables is deprecated|R:RelativeLeaf|",
            "D:Use of \"static\" in callables is deprecated|L:RelativeLeaf|"
        )
    );
}

#[test]
fn relative_instance_callbacks_cover_all_consumers_and_callable_hint() {
    assert_eq!(
        run_php(
            r#"<?php
set_error_handler(function ($level, $message) { echo 'D:', $message, '|'; return true; });
class ConsumerBase {
    public function value($ignored = null) { echo 'B:', get_class($this), '|'; }
}
class ConsumerMiddle extends ConsumerBase {
    public function value($ignored = null) { echo 'M:', get_class($this), '|'; }
    private function typed(callable $callback) { echo 'T|'; }
    public function probe() {
        foreach (['self::value', 'parent::value', 'static::value'] as $callback) {
            call_user_func($callback);
            call_user_func_array($callback, []);
            array_map($callback, [1]);
            Closure::fromCallable($callback)();
            $this->typed($callback);
        }
    }
}
class ConsumerLeaf extends ConsumerMiddle {
    public function value($ignored = null) { echo 'L:', get_class($this), '|'; }
}
(new ConsumerLeaf)->probe();
"#,
        ),
        concat!(
            "D:Use of \"self\" in callables is deprecated|M:ConsumerLeaf|",
            "D:Use of \"self\" in callables is deprecated|M:ConsumerLeaf|",
            "D:Use of \"self\" in callables is deprecated|M:ConsumerLeaf|",
            "D:Use of \"self\" in callables is deprecated|M:ConsumerLeaf|",
            "D:Use of \"self\" in callables is deprecated|T|",
            "D:Use of \"parent\" in callables is deprecated|B:ConsumerLeaf|",
            "D:Use of \"parent\" in callables is deprecated|B:ConsumerLeaf|",
            "D:Use of \"parent\" in callables is deprecated|B:ConsumerLeaf|",
            "D:Use of \"parent\" in callables is deprecated|B:ConsumerLeaf|",
            "D:Use of \"parent\" in callables is deprecated|T|",
            "D:Use of \"static\" in callables is deprecated|L:ConsumerLeaf|",
            "D:Use of \"static\" in callables is deprecated|L:ConsumerLeaf|",
            "D:Use of \"static\" in callables is deprecated|L:ConsumerLeaf|",
            "D:Use of \"static\" in callables is deprecated|L:ConsumerLeaf|",
            "D:Use of \"static\" in callables is deprecated|T|"
        )
    );
}

#[test]
fn qualified_legacy_callbacks_keep_receiver_visibility_and_errors() {
    assert_eq!(
        run_php(
            r#"<?php
set_error_handler(function ($level, $message) { echo 'D:', $message, '|'; return true; });
class QualifiedBase {
    private function inheritedPrivate() {}
}
class QualifiedMiddle extends QualifiedBase {
    private function hidden() { echo 'hidden|'; }
    protected function guarded() { echo 'guarded|'; }
    public function probe() {
        call_user_func_array([$this, 'parent::hidden'], []);
        call_user_func_array([$this, 'parent::guarded'], []);
        try { call_user_func_array([$this, 'parent::inheritedPrivate'], []); }
        catch (TypeError $error) { echo $error->getMessage(), '|'; }
        try { call_user_func_array([$this, 'self::missing'], []); }
        catch (TypeError $error) { echo $error->getMessage(), '|'; }
    }
}
class QualifiedLeaf extends QualifiedMiddle {}
(new QualifiedLeaf)->probe();
"#,
        ),
        concat!(
            "D:Callables of the form [\"QualifiedLeaf\", \"parent::hidden\"] are deprecated|hidden|",
            "D:Callables of the form [\"QualifiedLeaf\", \"parent::guarded\"] are deprecated|guarded|",
            "D:Callables of the form [\"QualifiedLeaf\", \"parent::inheritedPrivate\"] are deprecated|",
            "call_user_func_array(): Argument #1 ($callback) must be a valid callback, cannot access private method QualifiedMiddle::inheritedPrivate()|",
            "D:Callables of the form [\"QualifiedLeaf\", \"self::missing\"] are deprecated|",
            "call_user_func_array(): Argument #1 ($callback) must be a valid callback, class QualifiedLeaf does not have a method \"missing\"|"
        )
    );
}

#[test]
fn trait_parent_callback_uses_the_selected_composition_once() {
    assert_eq!(
        run_php(
            r#"<?php
set_error_handler(function ($level, $message) { echo 'D:', $message, '|'; return true; });
trait RelativeTrait {
    public function visit() {
        echo __CLASS__, '|';
        if (is_callable(['parent', __FUNCTION__])) { parent::visit(); }
    }
}
class TraitParent { use RelativeTrait; }
class TraitChild extends TraitParent { use RelativeTrait; }
(new TraitChild)->visit();
"#,
        ),
        "TraitChild|D:Use of \"parent\" in callables is deprecated|TraitParent|"
    );
}

#[test]
fn relative_callback_deprecation_exception_cleans_pending_call() {
    assert_eq!(
        run_php(
            r#"<?php
class DeprecatedCallbackCleanup {
    public static function target() { echo 'wrong'; }
    public static function probe() {
        set_error_handler(function () { throw new Exception('deprecated'); });
        try { call_user_func('self::target'); }
        catch (Throwable $error) {
            echo get_class($error), ':', $error->getMessage(), ':',
                get_class($error->getPrevious()), '|';
        }
        restore_error_handler();
        call_user_func([self::class, 'target']);
        echo 'ok';
    }
}
DeprecatedCallbackCleanup::probe();
"#,
        ),
        "TypeError:call_user_func(): Argument #1 ($callback) must be a valid callback, (null):Exception|wrongok"
    );
}

#[test]
fn invalid_relative_callback_without_class_scope_is_not_deprecated() {
    assert_eq!(
        run_php(
            r#"<?php
set_error_handler(function ($level, $message) { echo 'unexpected'; return true; });
echo is_callable('self::missing') ? 'yes' : 'no';
"#,
        ),
        "no"
    );
}

#[test]
fn scoped_static_syntax_prefers_live_instance_magic_call() {
    assert_eq!(
        run_php(
            r#"<?php
class ScopedMagicDispatch {
    public function __call($method, $arguments) {
        echo 'instance:', $method, ':', get_class($this), '|';
    }
    public static function __callStatic($method, $arguments) {
        echo 'static:', $method, '|';
    }
    public function probe() {
        self::relativeMissing();
        ScopedMagicDispatch::explicitMissing();
    }
}
(new ScopedMagicDispatch)->probe();
ScopedMagicDispatch::outsideMissing();
"#,
        ),
        concat!(
            "instance:relativeMissing:ScopedMagicDispatch|",
            "instance:explicitMissing:ScopedMagicDispatch|",
            "static:outsideMissing|"
        )
    );
}

#[test]
fn relative_shutdown_callback_resolves_before_its_scope_leaves() {
    assert_eq!(
        run_php(
            r#"<?php
set_error_handler(function ($level, $message) { echo 'D:', $message, '|'; return true; });
class RelativeShutdown {
    public static function register() {
        register_shutdown_function(['self', 'finish']);
    }
    public static function finish() { echo 'shutdown'; }
}
RelativeShutdown::register();
echo 'body|';
"#,
        ),
        "D:Use of \"self\" in callables is deprecated|body|shutdown"
    );
}
