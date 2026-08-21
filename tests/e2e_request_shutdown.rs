mod common;

use common::run_php;
use std::process::Command;

#[test]
fn shutdown_preentry_errors_have_the_inactive_file_origin_and_internal_trace() {
    for (callback, class, message) in [
        (
            "explode",
            "ArgumentCountError",
            "explode() expects at least 2 arguments, 0 given",
        ),
        (
            "get_defined_vars",
            "Error",
            "Cannot call get_defined_vars() dynamically",
        ),
    ] {
        let source = format!("register_shutdown_function('{callback}');");
        let output = Command::new(env!("CARGO_BIN_EXE_rphp"))
            .args(["-r", &source])
            .output()
            .expect("RPHP CLI must execute the pre-entry shutdown error specimen");

        assert_eq!(output.status.code(), Some(255), "{callback}");
        assert_eq!(String::from_utf8(output.stdout).unwrap(), "", "{callback}");
        assert_eq!(
            String::from_utf8(output.stderr).unwrap(),
            format!(
                concat!(
                    "\nFatal error: Uncaught {class}: {message} in [no active file]:0\n",
                    "Stack trace:\n",
                    "#0 [internal function]: {callback}()\n",
                    "#1 {{main}}\n",
                    "  thrown in [no active file] on line 0\n",
                ),
                class = class,
                message = message,
                callback = callback,
            ),
            "{callback}",
        );
    }
}

#[test]
fn shutdown_callback_owners_are_released_after_the_fifo_finishes() {
    assert_eq!(
        run_php(
            r#"<?php
class SuccessfulCallbackOwner {
    public function __construct(public string $name) {
        register_shutdown_function([$this, 'shutdown']);
    }
    public function shutdown(): void { echo "callback:$this->name|"; }
    public function __destruct() { echo "destruct:$this->name|"; }
}
new SuccessfulCallbackOwner('first');
new SuccessfulCallbackOwner('second');
echo "body|";
"#,
        ),
        concat!(
            "body|",
            "callback:first|callback:second|",
            "destruct:first|destruct:second|",
        )
    );
}

#[test]
fn shutdown_callback_exit_stops_the_fifo_and_releases_pending_owners() {
    let source = r#"
class CallbackOwner {
    public function __construct(public string $name, public bool $stop = false) {
        register_shutdown_function([$this, 'shutdown']);
    }
    public function shutdown(): void {
        echo "callback:$this->name|";
        if ($this->stop) {
            exit(7);
        }
    }
    public function __destruct() { echo "destruct:$this->name|"; }
}
new CallbackOwner('first', true);
new CallbackOwner('second');
echo "body|";
"#;
    let output = Command::new(env!("CARGO_BIN_EXE_rphp"))
        .args(["-r", source])
        .output()
        .expect("RPHP CLI must execute the shutdown exit specimen");

    assert_eq!(output.status.code(), Some(7));
    assert_eq!(
        String::from_utf8(output.stdout).unwrap(),
        "body|callback:first|destruct:first|destruct:second|"
    );
    assert_eq!(String::from_utf8(output.stderr).unwrap(), "");
}

#[test]
fn unhandled_shutdown_exception_still_releases_pending_callback_owners() {
    let source = r#"
class CallbackThrowOwner {
    public function __construct(private string $name) {}
    public function __destruct() { echo "destruct:", $this->name, "|"; }
}
$first = new CallbackThrowOwner('first');
$pending = new CallbackThrowOwner('pending');
register_shutdown_function(function () use ($first): void {
    echo "callback:first|";
    throw new Exception('callback');
});
register_shutdown_function(function () use ($pending): void {
    echo "callback:pending|";
});
unset($first, $pending);
echo "body|";
"#;
    let output = Command::new(env!("CARGO_BIN_EXE_rphp"))
        .args(["-r", source])
        .output()
        .expect("RPHP CLI must execute the shutdown exception specimen");

    assert_eq!(output.status.code(), Some(255));
    assert_eq!(
        String::from_utf8(output.stdout).unwrap(),
        concat!("body|callback:first|", "destruct:first|destruct:pending|",)
    );
    assert!(
        String::from_utf8(output.stderr)
            .unwrap()
            .contains("Fatal error: Uncaught Exception: callback")
    );
}

#[test]
fn request_shutdown_orders_main_class_and_function_static_destructors() {
    assert_eq!(
        run_php(
            r#"<?php
class StoredOwner {
    public function __construct(public string $name) {}
    public function __destruct() { echo "destruct:$this->name|"; }
}
class StaticStore { public static $value; }
function installFunctionStatic() {
    static $value;
    $value = new StoredOwner('function-static');
}
StaticStore::$value = new StoredOwner('class-static');
installFunctionStatic();
$global = new StoredOwner('main');
register_shutdown_function(static fn() => print 'callback|');
echo "body|";
"#,
        ),
        concat!(
            "body|callback|",
            "destruct:main|",
            "destruct:class-static|",
            "destruct:function-static|",
        )
    );
}

#[test]
fn static_destructors_see_their_cell_and_reach_a_cross_storage_fixed_point() {
    assert_eq!(
        run_php(
            r#"<?php
class CrossStorageOwner {
    public static $classOwner;
    public function __construct(public string $name) {}
    public function __destruct() {
        echo "destruct:$this->name:", get_debug_type(self::$classOwner), "|";
        if ($this->name === 'function') {
            self::$classOwner = new self('spawned-class');
        }
    }
}
function installCrossFunctionStatic() {
    static $owner;
    $owner = new CrossStorageOwner('function');
}
installCrossFunctionStatic();
echo "body|";
"#,
        ),
        concat!(
            "body|",
            "destruct:function:null|",
            "destruct:spawned-class:CrossStorageOwner|",
        )
    );
}

#[test]
fn shutdown_static_destructor_exception_uses_the_active_handler() {
    assert_eq!(
        run_php(
            r#"<?php
class ShutdownDestructorFailure extends Exception {}
class ThrowingShutdownOwner {
    public function __destruct() {
        echo "destruct|";
        throw new ShutdownDestructorFailure('shutdown');
    }
}
class ThrowingStaticStore { public static $value; }
ThrowingStaticStore::$value = new ThrowingShutdownOwner();
set_exception_handler(static function (Throwable $error): void {
    echo 'handler:', $error::class, ':', $error->getMessage(), '|';
});
echo "body|";
"#,
        ),
        "body|destruct|handler:ShutdownDestructorFailure:shutdown|"
    );
}

#[test]
fn shutdown_static_destructor_has_an_internal_trace_boundary() {
    let source = r#"
class UnhandledStaticShutdownOwner {
    public static $owner;
    public function __destruct() {
        throw new Exception('shutdown');
    }
}
UnhandledStaticShutdownOwner::$owner = new UnhandledStaticShutdownOwner();
"#;
    let output = Command::new(env!("CARGO_BIN_EXE_rphp"))
        .args(["-r", source])
        .output()
        .expect("RPHP CLI must execute the static destructor trace specimen");

    assert_eq!(output.status.code(), Some(255));
    assert_eq!(String::from_utf8(output.stdout).unwrap(), "");
    let stderr = String::from_utf8(output.stderr).unwrap();
    assert!(stderr.contains("Fatal error: Uncaught Exception: shutdown"));
    assert!(stderr.contains(concat!(
        "Stack trace:\n",
        "#0 [internal function]: UnhandledStaticShutdownOwner->__destruct()\n",
        "#1 {main}\n",
    )));
}

#[test]
fn object_shared_by_main_and_static_storage_destructs_once() {
    assert_eq!(
        run_php(
            r#"<?php
class SharedShutdownOwner {
    public static $owner;
    public function __destruct() { echo "destruct|"; }
}
$owner = new SharedShutdownOwner();
SharedShutdownOwner::$owner = $owner;
echo "body|";
"#,
        ),
        "body|destruct|"
    );
}
