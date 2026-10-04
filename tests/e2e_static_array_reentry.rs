mod common;

use common::run_php;

#[test]
fn dynamic_static_owner_capture_keeps_class_identity_without_delaying_destruction() {
    assert_eq!(
        run_php(
            r#"<?php
class LifetimeOwner {
    public static array $data = [];
    public function __destruct() { echo "drop|"; }
}
function replace_local_owner(): void {
    $owner = new LifetimeOwner();
    $rhs = function () use (&$owner): int {
        echo "rhs-start|";
        $owner = null;
        echo "rhs-end|";
        return 7;
    };
    $owner::$data['leaf'] = $rhs();
}
replace_local_owner();
echo json_encode(LifetimeOwner::$data), "\n";
function lifetime_rhs(): int {
    echo "rhs-start|";
    echo "rhs-end|";
    return 7;
}
LifetimeOwner::$data = [];
(new LifetimeOwner())::$data['leaf'] = lifetime_rhs();
echo json_encode(LifetimeOwner::$data), "\n";
function temporary_owner(): LifetimeOwner { return new LifetimeOwner(); }
LifetimeOwner::$data = [];
temporary_owner()::$data['leaf'] = lifetime_rhs();
echo json_encode(LifetimeOwner::$data), "\n";
"#,
        ),
        concat!(
            "rhs-start|drop|rhs-end|{\"leaf\":7}\n",
            "drop|rhs-start|rhs-end|{\"leaf\":7}\n",
            "drop|rhs-start|rhs-end|{\"leaf\":7}\n",
        )
    );
}

#[test]
fn dynamic_static_owner_resolution_precedes_member_keys_and_rhs() {
    assert_eq!(
        run_php(
            r#"<?php
function member_name(): string { echo "member|"; return 'data'; }
function member_key(): string { echo "key|"; return 'leaf'; }
function member_rhs(): int { echo "rhs|"; return 7; }
foreach ([null, 7, 'UnknownOwner'] as $owner) {
    try { $owner::${member_name()}[member_key()] = member_rhs(); }
    catch (Throwable $error) { echo $error->getMessage(), "\n"; }
}
spl_autoload_register(function (string $name): void {
    echo "load:$name|";
    $GLOBALS['owner'] = stdClass::class;
    eval('class LoadedOwner { public static array $data = []; }');
});
$owner = 'LoadedOwner';
$owner::${member_name()}[member_key()] = member_rhs();
echo json_encode(LoadedOwner::$data), "\n";
"#,
        ),
        concat!(
            "Class name must be a valid object or a string\n",
            "Class name must be a valid object or a string\n",
            "Class \"UnknownOwner\" not found\n",
            "load:LoadedOwner|member|key|rhs|{\"leaf\":7}\n",
        )
    );
}

#[test]
fn property_dimension_writes_preserve_rhs_changes_without_reference_promotion() {
    assert_eq!(
        run_php(
            r#"<?php
class PropertyCache {
    public static array $shared = [];
    public array $data = ['row' => ['seed' => 1]];
    public static function static_child(string $root): object {
        self::$shared[$root]['child'] = new stdClass();
        return new stdClass();
    }
    public static function static_parent(string $root, string $leaf): object {
        return self::$shared[$root][$leaf] = self::static_child($root);
    }
    public function child(string $root): object {
        $this->data[$root]['child'] = new stdClass();
        return new stdClass();
    }
    public function parent(string $root, string $leaf): object {
        return $this->data[$root][$leaf] = $this->child($root);
    }
}
foreach ([false, true] as $seeded) {
    PropertyCache::$shared = $seeded ? ['row' => ['seed' => 1]] : [];
    $copy = PropertyCache::$shared;
    PropertyCache::static_parent('row', 'parent');
    echo json_encode([array_keys(PropertyCache::$shared['row']), array_keys($copy['row'] ?? [])]), "\n";
}
$object = new PropertyCache();
$copy = $object->data;
$object->parent('row', 'parent');
echo json_encode([array_keys($object->data['row']), array_keys($copy['row'])]), "\n";
"#,
        ),
        "[[\"child\",\"parent\"],[]]\n[[\"seed\",\"child\",\"parent\"],[\"seed\"]]\n[[\"seed\",\"child\",\"parent\"],[\"seed\"]]\n"
    );
}

#[test]
fn nested_static_writes_preserve_callback_changes_and_cow() {
    assert_eq!(
        run_php(
            r#"<?php
class StaticCache {
    public static array $data = [];
    public static function child(): object {
        self::$data['row']['child'] = new stdClass();
        return new stdClass();
    }
    public static function parent(): object {
        return self::$data['row']['parent'] = self::child();
    }
}
foreach ([false, true] as $seeded) {
    StaticCache::$data = $seeded ? ['row' => ['seed' => 1]] : [];
    $copy = StaticCache::$data;
    $alias = &StaticCache::$data;
    $result = StaticCache::parent();
    echo json_encode([array_keys(StaticCache::$data['row']), array_keys($copy['row'] ?? []), $alias === StaticCache::$data, $result === StaticCache::$data['row']['parent']]), "\n";
    unset($alias);
}
"#,
        ),
        "[[\"child\",\"parent\"],[],true,true]\n[[\"seed\",\"child\",\"parent\"],[\"seed\"],true,true]\n"
    );
}

#[test]
fn nested_static_writes_use_replaced_storage_and_preserve_throwing_side_effects() {
    assert_eq!(
        run_php(
            r#"<?php
class ReplacedStorage { public static $data = []; }
function replace_storage(string $mode): int {
    if ($mode === 'root') ReplacedStorage::$data = ['row' => ['fresh' => 2]];
    if ($mode === 'row') ReplacedStorage::$data['row'] = ['fresh' => 2];
    if ($mode === 'scalar') ReplacedStorage::$data = 9;
    if ($mode === 'throw') {
        ReplacedStorage::$data['row']['child'] = 2;
        throw new Exception('stop');
    }
    return 7;
}
foreach (['root', 'row', 'scalar', 'throw'] as $mode) {
    ReplacedStorage::$data = ['row' => ['seed' => 1]];
    $copy = ReplacedStorage::$data;
    try {
        $result = ReplacedStorage::$data['row']['parent'] = replace_storage($mode);
        echo "result:$result\n";
    } catch (Throwable $error) {
        echo get_class($error), ':', $error->getMessage(), "\n";
    }
    echo json_encode([ReplacedStorage::$data, $copy]), "\n";
}
"#,
        ),
        concat!(
            "result:7\n[{\"row\":{\"fresh\":2,\"parent\":7}},{\"row\":{\"seed\":1}}]\n",
            "result:7\n[{\"row\":{\"fresh\":2,\"parent\":7}},{\"row\":{\"seed\":1}}]\n",
            "Error:Cannot use a scalar value as an array\n[9,{\"row\":{\"seed\":1}}]\n",
            "Exception:stop\n[{\"row\":{\"seed\":1,\"child\":2}},{\"row\":{\"seed\":1}}]\n",
        )
    );
}

#[test]
fn static_dimension_writes_retain_dynamic_target_identity_and_called_scope() {
    assert_eq!(
        run_php(
            r#"<?php
class FirstTarget { public static array $data = []; public static array $other = []; }
class SecondTarget { public static array $data = []; }
$owner = FirstTarget::class;
$key = 'row';
function change_target(): int {
    $GLOBALS['owner'] = SecondTarget::class;
    $GLOBALS['key'] = 'changed';
    FirstTarget::$data['row']['child'] = 1;
    return 7;
}
$owner::$data[$key]['parent'] = change_target();
echo json_encode([FirstTarget::$data, FirstTarget::$other, SecondTarget::$data]), "\n";
class ScopeTarget {
    public static array $data = [];
    public static function child(): int { static::$data['row']['child'] = 2; return 8; }
    public static function parent(): int { return static::$data['row']['parent'] = static::child(); }
}
class ScopeChild extends ScopeTarget { public static array $data = []; }
ScopeChild::parent();
echo json_encode([ScopeTarget::$data, ScopeChild::$data]), "\n";
"#,
        ),
        "[{\"row\":{\"child\":1},\"changed\":{\"parent\":7}},[],[]]\n[[],{\"row\":{\"child\":2,\"parent\":8}}]\n"
    );
}

#[test]
fn static_dimension_capability_errors_follow_keys_and_rhs() {
    assert_eq!(
        run_php(
            r#"<?php
class ErrorTarget {
    public static array $uninitialized;
    private static array $hidden = [];
}
function dimension_key(): string { echo "key\n"; return 'row'; }
function dimension_rhs(): int { echo "rhs\n"; return 7; }
try { ErrorTarget::$missing[dimension_key()]['leaf'] = dimension_rhs(); }
catch (Error $error) { echo $error->getMessage(), "\n"; }
try { ErrorTarget::$hidden[dimension_key()]['leaf'] = dimension_rhs(); }
catch (Error $error) { echo $error->getMessage(), "\n"; }
try { AbsentTarget::$data[dimension_key()]['leaf'] = dimension_rhs(); }
catch (Error $error) { echo $error->getMessage(), "\n"; }
ErrorTarget::$uninitialized[dimension_key()]['leaf'] = dimension_rhs();
echo json_encode(ErrorTarget::$uninitialized), "\n";
"#,
        ),
        concat!(
            "key\nrhs\nAccess to undeclared static property ErrorTarget::$missing\n",
            "key\nrhs\nCannot access private property ErrorTarget::$hidden\n",
            "key\nrhs\nClass \"AbsentTarget\" not found\n",
            "key\nrhs\n{\"row\":{\"leaf\":7}}\n",
        )
    );
}
