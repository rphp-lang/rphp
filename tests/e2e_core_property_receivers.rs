mod common;
use common::run_php;

#[test]
fn scalar_property_read_diagnostics_preserve_boolean_values() {
    assert_eq!(
        run_php(
            r#"<?php
set_error_handler(function($level, $message) { echo $message, "\n"; });
foreach ([true, false, 0, '', [], null] as $receiver) {
    var_dump($receiver->p);
}
"#
        ),
        concat!(
            "Attempt to read property \"p\" on true\nNULL\n",
            "Attempt to read property \"p\" on false\nNULL\n",
            "Attempt to read property \"p\" on int\nNULL\n",
            "Attempt to read property \"p\" on string\nNULL\n",
            "Attempt to read property \"p\" on array\nNULL\n",
            "Attempt to read property \"p\" on null\nNULL\n",
        )
    );
}

#[test]
fn static_declarations_accessed_through_an_object_warn_without_touching_static_storage() {
    assert_eq!(
        run_php(
            r#"<?php
class ParentStatic {
    private static $hidden = 1;
    protected static $guarded = 2;
    public static $visible = 3;
    public $instance = 4;
    function inspect() {
        var_dump($this->hidden, $this->guarded, $this->visible, $this->instance);
    }
}

#[AllowDynamicProperties]
class ChildStatic extends ParentStatic {}
set_error_handler(function($level, $message) { echo $level, ':', $message, "\n"; });
$child = new ChildStatic;
$child->inspect();
$child->visible = 7;
var_dump($child->visible, ChildStatic::$visible);
class MagicStatic {
    public static $visible = 1;
    function __get($name) { echo "get:$name\n"; return 5; }
    function __set($name, $value) { echo "set:$name=$value\n"; }
}
$magic = new MagicStatic;
var_dump($magic->visible);
$magic->visible = 6;
"#
        ),
        concat!(
            "8:Accessing static property ChildStatic::$hidden as non static\n",
            "2:Undefined property: ChildStatic::$hidden\n",
            "8:Accessing static property ChildStatic::$guarded as non static\n",
            "2:Undefined property: ChildStatic::$guarded\n",
            "8:Accessing static property ChildStatic::$visible as non static\n",
            "2:Undefined property: ChildStatic::$visible\n",
            "NULL\nNULL\nNULL\nint(4)\n",
            "8:Accessing static property ChildStatic::$visible as non static\n",
            "8:Accessing static property ChildStatic::$visible as non static\n",
            "int(7)\nint(3)\n",
            "get:visible\nint(5)\nset:visible=6\n",
        )
    );
}

#[test]
fn throwing_notice_handler_on_object_static_assignment_completes_dynamic_write_and_leaves_static_slot_unchanged()
 {
    assert_eq!(
        run_php(
            r#"<?php
#[AllowDynamicProperties]
class StaticHolder {
    public static $slot = 'static-original';
}
$obj = new StaticHolder;
set_error_handler(function($level, $message) use ($obj) {
    echo "handler:", $level, ':', $message, "\n";
    var_dump(get_object_vars($obj));
    echo "during-handler-static:", StaticHolder::$slot, "\n";
    throw new RuntimeException('notice-handler-threw');
});
try {
    $obj->slot = 'dynamic-written';
} catch (RuntimeException $e) {
    echo "caught:", $e->getMessage(), "\n";
}
restore_error_handler();
var_dump(get_object_vars($obj), StaticHolder::$slot);
"#
        ),
        concat!(
            "handler:8:Accessing static property StaticHolder::$slot as non static\n",
            "array(0) {\n}\n",
            "during-handler-static:static-original\n",
            "caught:notice-handler-threw\n",
            "array(1) {\n  [\"slot\"]=>\n  string(15) \"dynamic-written\"\n}\n",
            "string(15) \"static-original\"\n",
        )
    );
}

#[test]
fn throwing_static_access_notice_preserves_direct_write_validation_and_destructor_order() {
    assert_eq!(
        run_php(
            r#"<?php
class PlainReceiver { public static $slot = 'static'; }
$plain = new PlainReceiver;
set_error_handler(function($level, $message) use ($plain) {
    echo "plain-handler:$message\n";
    var_dump(get_object_vars($plain));
    throw new Exception('plain-handler');
});
try { $plain->slot = 'dynamic'; } catch (Throwable $error) {
    echo 'plain-caught:', $error->getMessage(), "\n";
}
restore_error_handler();
var_dump(get_object_vars($plain));

class DestructingValue {
    public function __destruct() { echo "destruct\n"; throw new Exception('destructor'); }
}
#[AllowDynamicProperties]
class ExistingReceiver { public static $slot = 'static'; }
$existing = new ExistingReceiver;
@$existing->slot = new DestructingValue;
set_error_handler(function($level, $message) use ($existing) {
    echo "existing-handler:$message\n";
    var_dump(array_keys(get_object_vars($existing)));
    throw new Exception('existing-handler');
});
try { $existing->slot = 3; } catch (Throwable $error) {
    echo 'existing-caught:', $error->getMessage(), "\n";
    echo 'existing-previous:', $error->getPrevious()?->getMessage(), "\n";
}
restore_error_handler();
var_dump(get_object_vars($existing));

#[AllowDynamicProperties]
class ReferencedReceiver { public static $slot = 'static'; }
class TypedOwner { public int $value = 1; }
$referenced = unserialize('O:18:"ReferencedReceiver":1:{s:4:"slot";i:0;}');
$typed = new TypedOwner;
@$referenced->slot =& $typed->value;
set_error_handler(function($level, $message) {
    echo "reference-handler:$message\n";
    throw new Exception('reference-handler');
});
try { $referenced->slot = 'bad'; } catch (Throwable $error) {
    echo 'reference-caught:', get_class($error), ':', $error->getMessage(), "\n";
    echo 'reference-previous:', $error->getPrevious()?->getMessage(), "\n";
}
restore_error_handler();
var_dump($typed->value, get_object_vars($referenced));
"#
        ),
        concat!(
            "plain-handler:Accessing static property PlainReceiver::$slot as non static\n",
            "array(0) {\n}\nplain-caught:plain-handler\n",
            "array(1) {\n  [\"slot\"]=>\n  string(7) \"dynamic\"\n}\n",
            "existing-handler:Accessing static property ExistingReceiver::$slot as non static\n",
            "array(1) {\n  [0]=>\n  string(4) \"slot\"\n}\n",
            "destruct\nexisting-caught:destructor\n",
            "existing-previous:existing-handler\n",
            "array(1) {\n  [\"slot\"]=>\n  int(3)\n}\n",
            "reference-handler:Accessing static property ReferencedReceiver::$slot as non static\n",
            "reference-caught:TypeError:Cannot assign string to reference held by property TypedOwner::$value of type int\n",
            "reference-previous:reference-handler\n",
            "int(1)\narray(1) {\n  [\"slot\"]=>\n  &int(1)\n}\n",
        )
    );
}

#[test]
fn inherited_instance_slot_takes_priority_over_private_static_declaration() {
    assert_eq!(
        run_php(
            r#"<?php
class StaticOwner {
    private static $value = 1;
    function update() {
        $this->value = 2;
        var_dump($this->value, self::$value);
    }
}
class InstanceChild extends StaticOwner { public $value; }
(new InstanceChild)->update();
"#
        ),
        "int(2)\nint(1)\n"
    );
}
