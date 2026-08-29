mod common;

use common::run_php;

#[test]
fn magic_bridge_and_indirect_hook_results_preserve_the_lvalue_transaction() {
    assert_eq!(
        run_php(
            r#"<?php
class HookTransaction {
    private array $storage = [];
    private object $nodeStorage;
    public function __construct() { $this->nodeStorage = (object) ['count' => 1]; }
    private int $secret {
        get { echo "secret:get\n"; return 7; }
        set { echo "secret:set:$value\n"; }
    }
    public array $copy { get { echo "copy:get\n"; return $this->storage; } }
    public array $alias { &get { echo "alias:get\n"; return $this->storage; } }
    public object $node { get { echo "node:get\n"; return $this->nodeStorage; } }
    public function __get(string $name): mixed {
        echo "magic:get:$name\n";
        return $this->{$name};
    }
    public function __set(string $name, mixed $value): void {
        echo "magic:set:$name\n";
        $this->{$name} = $value;
    }
}
$object = new HookTransaction();
var_dump($object->secret);
$object->secret = 9;
try { $object->copy[] = 'detached'; }
catch (Error $error) { echo $error->getMessage(), "\n"; }
$object->alias[] = 'shared';
$object->node->count++;
var_dump($object->alias, $object->node);
"#,
        ),
        concat!(
            "magic:get:secret\nsecret:get\nint(7)\n",
            "magic:set:secret\nsecret:set:9\n",
            "copy:get\nIndirect modification of HookTransaction::$copy is not allowed\n",
            "alias:get\nnode:get\nalias:get\nnode:get\n",
            "array(1) {\n  [0]=>\n  string(6) \"shared\"\n}\n",
            "object(stdClass)#2 (1) {\n  [\"count\"]=>\n  int(2)\n}\n",
        )
    );
}

#[test]
fn foreach_dump_and_serialize_share_virtual_and_backed_storage_rules() {
    assert_eq!(
        run_php(
            r#"<?php
#[AllowDynamicProperties]
class HookProjection {
    public string $plain = 'plain';
    private string $backing = 'virtual';
    public string $virtual {
        get { echo "virtual:get\n"; return $this->backing; }
        set { echo "virtual:set:$value\n"; $this->backing = $value; }
    }
    public string $stored = 'stored' {
        get { echo "stored:get\n"; return $this->stored; }
        set { echo "stored:set:$value\n"; $this->stored = $value; }
    }
    public $sink { set { echo "sink:set:$value\n"; } }
    public function __construct() { $this->dynamic = 'dynamic'; }
}
$object = new HookProjection();
foreach ($object as $name => $value) {
    echo "$name=$value\n";
    $object->{$name} = strtoupper($value);
}
var_dump($object);
echo serialize($object), "\n";
"#,
        ),
        concat!(
            "plain=plain\nvirtual:get\nvirtual=virtual\nvirtual:set:VIRTUAL\n",
            "stored:get\nstored=stored\nstored:set:STORED\ndynamic=dynamic\n",
            "object(HookProjection)#1 (4) {\n",
            "  [\"plain\"]=>\n  string(5) \"PLAIN\"\n",
            "  [\"backing\":\"HookProjection\":private]=>\n  string(7) \"VIRTUAL\"\n",
            "  [\"stored\"]=>\n  string(6) \"STORED\"\n",
            "  [\"dynamic\"]=>\n  string(7) \"DYNAMIC\"\n}\n",
            "O:14:\"HookProjection\":4:{s:5:\"plain\";s:5:\"PLAIN\";",
            "s:23:\"\0HookProjection\0backing\";s:7:\"VIRTUAL\";",
            "s:6:\"stored\";s:6:\"STORED\";s:7:\"dynamic\";s:7:\"DYNAMIC\";}\n",
        )
    );
}

#[test]
fn private_and_parent_hooks_keep_their_declaring_scope() {
    assert_eq!(
        run_php(
            r#"<?php
class HookBase {
    private $hidden {
        get { echo "base:hidden:get\n"; return 10; }
        set { echo "base:hidden:set:$value\n"; }
    }
    public $score = 1 {
        get { echo "base:score:get\n"; return $this->score; }
        set { echo "base:score:set:$value\n"; $this->score = $value; }
    }
    public function touch(): void {
        var_dump($this->hidden);
        $this->hidden = 11;
    }
}
class HookChild extends HookBase {
    public $hidden {
        get { echo "child:hidden:get\n"; return 20; }
        set { echo "child:hidden:set:$value\n"; }
    }
    public $score {
        get { echo "child:score:get\n"; return parent::$score::get() + 1; }
        set { echo "child:score:set:$value\n"; parent::$score::set($value + 1); }
    }
}
$child = new HookChild();
$child->touch();
var_dump($child->hidden);
$child->hidden = 21;
var_dump($child->score);
$child->score = 30;
var_dump($child->score);
"#,
        ),
        concat!(
            "base:hidden:get\nint(10)\nbase:hidden:set:11\n",
            "child:hidden:get\nint(20)\nchild:hidden:set:21\n",
            "child:score:get\nbase:score:get\nint(1)\n",
            "child:score:set:30\nbase:score:set:31\n",
            "child:score:get\nbase:score:get\nint(32)\n",
        )
    );
}

#[test]
fn ordinary_serialization_excludes_virtual_slots_and_rejects_virtual_input() {
    assert_eq!(
        run_php(
            r#"<?php
class HookWire {
    public int $stored = 1;
    public int $virtual { get => 2; }
    public int $sink { set {} }
}
$object = new HookWire();
echo serialize($object), "\n";
set_error_handler(function (int $severity, string $message): bool {
    echo "warning:$message\n";
    return true;
});
var_dump(unserialize('O:8:"HookWire":1:{s:7:"virtual";i:9;}'));
"#,
        ),
        concat!(
            "O:8:\"HookWire\":1:{s:6:\"stored\";i:1;}\n",
            "warning:unserialize(): Cannot unserialize value for virtual property HookWire::$virtual\n",
            "warning:unserialize(): Error at offset 32 of 37 bytes\n",
            "bool(false)\n",
        )
    );
}

#[test]
fn lazy_proxy_dispatches_the_set_hook_before_backing_initialization() {
    assert_eq!(
        run_php(
            r#"<?php
class LazyHookTarget {
    public int $value {
        set {
            echo "set:$value\n";
            $this->value = $value * 3;
        }
    }
}
$reflection = new ReflectionClass(LazyHookTarget::class);
$proxy = $reflection->newLazyProxy(function () {
    echo "initialize\n";
    return new LazyHookTarget();
});
$proxy->value = 4;
var_dump($proxy->value);
"#,
        ),
        "set:4\ninitialize\nint(12)\n"
    );
}
