mod common;

use common::run_php;
use std::process::Command;

#[test]
fn catch_entry_releases_replaced_and_uncaptured_throwables_before_the_body() {
    assert_eq!(
        run_php(
            r#"<?php
class CatchEntryDrop {
    public function __construct(private string $name) {}
    public function __destruct() {
        echo "drop:", $this->name, "\n";
        throw new RuntimeException($this->name);
    }
}

try {
    $slot = new CatchEntryDrop('retired');
    try { throw new Exception('incoming'); }
    catch (Throwable $slot) { echo "inner\n"; }
} catch (Throwable $error) {
    echo "caught:", $error->getMessage(), "\n";
    echo "previous:", $error->getPrevious()?->getMessage() ?? 'none', "\n";
}

class TraceLeaf {
    public function __destruct() { echo "trace-drop\n"; }
}
function traceFailure(#[SensitiveParameter] TraceLeaf $leaf): void {
    throw new Exception('trace');
}
try { traceFailure(new TraceLeaf); }
catch (Throwable) { echo "catch-body\n"; }
"#,
        ),
        concat!(
            "drop:retired\n",
            "caught:retired\n",
            "previous:none\n",
            "trace-drop\n",
            "catch-body\n",
        )
    );
}

#[test]
fn coalesce_assignment_releases_object_keys_inside_the_surrounding_try() {
    assert_eq!(
        run_php(
            r#"<?php
class ReleaseBag implements ArrayAccess {
    public function offsetExists($key): bool { return true; }
    public function &offsetGet($key): mixed {
        $value = ['leaf' => null];
        return $value;
    }
    public function offsetSet($key, $value): void {}
    public function offsetUnset($key): void {}
}
class TemporaryKey {
    public function __destruct() {
        echo "key-drop\n";
        throw new RuntimeException('key');
    }
}

$bag = new ReleaseBag;
try { $bag[new TemporaryKey]['leaf'] ??= 'value'; }
catch (Throwable $error) { echo "caught:", $error->getMessage(), "\n"; }
echo "done\n";
"#,
        ),
        "key-drop\ncaught:key\ndone\n"
    );
}

#[test]
fn detached_destructor_callbacks_unwind_local_objects_and_chain_replacements() {
    assert_eq!(
        run_php(
            r#"<?php
class NestedDrop {
    public static int $depth = 0;
    public function __destruct() {
        $depth = self::$depth++;
        echo "drop:", $depth, "\n";
        if ($depth === 0) {
            $child = new self;
            throw new RuntimeException('outer');
        }
        throw new RuntimeException('inner');
    }
}

try { new NestedDrop; }
catch (Throwable $error) {
    echo "caught:", $error->getMessage(), "\n";
    echo "previous:", $error->getPrevious()?->getMessage() ?? 'none', "\n";
}
echo "done\n";
"#,
        ),
        concat!(
            "drop:0\n",
            "drop:1\n",
            "caught:inner\n",
            "previous:outer\n",
            "done\n",
        )
    );
}

#[test]
fn shutdown_continues_after_each_handled_destructor_exception() {
    assert_eq!(
        run_php(
            r#"<?php
class ShutdownDrop {
    public function __construct(private string $name) {}
    public function __destruct() {
        echo "shutdown:", $this->name, "\n";
        throw new RuntimeException($this->name);
    }
}
set_exception_handler(function (Throwable $error) {
    echo "handled:", $error->getMessage(), "\n";
    echo "previous:", $error->getPrevious()?->getMessage() ?? 'none', "\n";
});
$first = new ShutdownDrop('first');
$second = new ShutdownDrop('second');
echo "body\n";
"#,
        ),
        concat!(
            "body\n",
            "shutdown:second\n",
            "handled:second\n",
            "previous:none\n",
            "shutdown:first\n",
            "handled:first\n",
            "previous:none\n",
        )
    );
}

#[test]
fn handler_stack_changes_inside_the_active_handler_survive_shutdown() {
    assert_eq!(
        run_php(
            r#"<?php
function consumeActiveHandler(Throwable $error): void {
    echo "handler:", $error->getMessage(), "\n";
    restore_exception_handler();
    restore_exception_handler();
}
set_exception_handler('consumeActiveHandler');
register_shutdown_function(static function (): void {
    var_dump(get_exception_handler());
});
throw new RuntimeException('active');
"#,
        ),
        "handler:active\nNULL\n"
    );
}

#[test]
fn unhandled_shutdown_local_destructor_replaces_without_a_previous_chain() {
    let source = r#"
class ShutdownNestedReplacement {
    public function __destruct() {
        static $outer = true;
        if ($outer) {
            $outer = false;
            $local = new self;
            throw new RuntimeException('outer');
        }
        throw new RuntimeException('inner');
    }
}
$root = new ShutdownNestedReplacement;
"#;
    let output = Command::new(env!("CARGO_BIN_EXE_rphp"))
        .args(["-r", source])
        .output()
        .expect("RPHP CLI must execute the nested shutdown specimen");

    assert_eq!(output.status.code(), Some(255));
    assert_eq!(String::from_utf8(output.stdout).unwrap(), "");
    let stderr = String::from_utf8(output.stderr).unwrap();
    assert!(stderr.contains("Uncaught RuntimeException: inner"));
    assert!(!stderr.contains("Next RuntimeException"));
    assert!(!stderr.contains("RuntimeException: outer"));
}

#[test]
fn request_roots_release_globals_then_constants_class_and_function_statics() {
    assert_eq!(
        run_php(
            r#"<?php
class StoredDrop {
    public function __construct(private string $name) {}
    public function __destruct() { echo "drop:", $this->name, "\n"; }
}
class StaticStore { public static mixed $value; }
function installFunctionStatic(): void {
    static $value;
    $value = new StoredDrop('function');
}
const STORED_OBJECT = new StoredDrop('constant');
StaticStore::$value = new StoredDrop('class');
installFunctionStatic();
$global = new StoredDrop('global');
echo "body\n";
"#,
        ),
        concat!(
            "body\n",
            "drop:global\n",
            "drop:constant\n",
            "drop:class\n",
            "drop:function\n",
        )
    );
}

#[test]
fn consumed_reference_return_does_not_alias_a_later_clone_write() {
    assert_eq!(
        run_php(
            r#"<?php
class ReferenceBox {
    public array $items = [1];
    public function &items(): array { return $this->items; }
}
$box = new ReferenceBox;
$copy = $box->items();
$clone = clone $box;
$clone->items = [];
echo json_encode($box->items), "\n";
echo json_encode($copy), "\n";
echo json_encode($clone->items), "\n";
"#,
        ),
        "[1]\n[1]\n[]\n"
    );
}
