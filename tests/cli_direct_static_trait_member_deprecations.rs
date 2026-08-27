use std::io::Write;
use std::process::{Command, Stdio};

fn run_stdin(source: &str) -> (i32, String, String) {
    let mut child = Command::new(env!("CARGO_BIN_EXE_rphp"))
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .expect("rphp subprocess should start");
    child
        .stdin
        .take()
        .expect("stdin should be piped")
        .write_all(source.as_bytes())
        .expect("source should be written");
    let output = child.wait_with_output().expect("rphp should finish");
    (
        output.status.code().expect("rphp should exit normally"),
        String::from_utf8(output.stdout).expect("stdout should be UTF-8"),
        String::from_utf8(output.stderr).expect("stderr should be UTF-8"),
    )
}

#[test]
fn direct_trait_access_warns_on_every_cache_hit_but_consumer_access_does_not() {
    let (status, stdout, stderr) = run_stdin(
        r#"<?php
$events = [];
set_error_handler(function ($level, $message) use (&$events) {
    $events[] = $message;
    return true;
});
trait SharedStatics {
    public static int $value = 1;
    public static function declared() {}
    public static function __callStatic($name, $arguments) {}
}
class Consumer { use SharedStatics; }
function exercise_trait() {
    SharedStatics::$value = 7;
    $copy = SharedStatics::$value;
    SharedStatics::declared();
    SharedStatics::missing();
}
exercise_trait();
exercise_trait();
Consumer::$value = 9;
$copy = Consumer::$value;
Consumer::declared();
Consumer::missing();
echo count($events), "\n", implode("\n", $events), "\n";
"#,
    );

    assert_eq!(status, 0);
    assert_eq!(stderr, "");
    let property = "Accessing static trait property SharedStatics::$value is deprecated, it should only be accessed on a class using the trait";
    let declared = "Calling static trait method SharedStatics::declared is deprecated, it should only be called on a class using the trait";
    let missing = "Calling static trait method SharedStatics::missing is deprecated, it should only be called on a class using the trait";
    assert_eq!(
        stdout,
        format!(
            "8\n{property}\n{property}\n{declared}\n{missing}\n{property}\n{property}\n{declared}\n{missing}\n"
        )
    );
}

#[test]
fn throwing_deprecation_handler_precedes_typed_failure_and_magic_dispatch() {
    let (status, stdout, stderr) = run_stdin(
        r#"<?php
set_error_handler(function ($level, $message) {
    throw new ErrorException($message, 0, $level);
});
trait GuardedStatics {
    public static self $typed;
    public static function __callStatic($name, $arguments) {
        echo "magic-ran\n";
    }
}
try {
    GuardedStatics::$typed = new stdClass;
} catch (ErrorException $error) {
    echo "property: ", $error->getMessage(), "\n";
}
try {
    GuardedStatics::unknown();
} catch (ErrorException $error) {
    echo "method: ", $error->getMessage(), "\n";
}
"#,
    );

    assert_eq!(status, 0);
    assert_eq!(stderr, "");
    assert_eq!(
        stdout,
        concat!(
            "property: Accessing static trait property GuardedStatics::$typed is deprecated, it should only be accessed on a class using the trait\n",
            "method: Calling static trait method GuardedStatics::unknown is deprecated, it should only be called on a class using the trait\n",
        )
    );
    assert!(!stdout.contains("magic-ran"));
    assert!(!stdout.contains("Cannot assign"));
}

#[test]
fn first_class_trait_callable_warns_at_creation_and_remains_callable() {
    let (status, stdout, stderr) = run_stdin(
        r#"<?php
$events = [];
set_error_handler(function ($level, $message) use (&$events) {
    $events[] = $message;
    return true;
});
trait CallableFragment {
    public static function double(int $value): int { return $value * 2; }
}
const JOB = CallableFragment::double(...);
echo count($events), ":", (JOB)(6), "\n", $events[0], "\n";
"#,
    );

    assert_eq!(status, 0);
    assert_eq!(stderr, "");
    assert_eq!(
        stdout,
        concat!(
            "1:12\n",
            "Calling static trait method CallableFragment::double is deprecated, it should only be called on a class using the trait\n",
        )
    );
}

#[test]
fn relative_static_access_uses_the_runtime_trait_or_consumer_scope() {
    let (status, stdout, stderr) = run_stdin(
        r#"<?php
$events = [];
set_error_handler(function ($level, $message) use (&$events) {
    $events[] = $message;
    return true;
});
trait RelativeStatics {
    public static int $value = 3;
    public static function inner() {}
    public static function outer() {
        $copy = self::$value;
        static::inner();
    }
}
class RelativeConsumer { use RelativeStatics; }
RelativeStatics::outer();
echo count($events), "\n";
$events = [];
RelativeConsumer::outer();
echo count($events), "\n";
"#,
    );

    assert_eq!(status, 0);
    assert_eq!(stdout, "3\n0\n");
    assert_eq!(stderr, "");
}

#[test]
fn rejected_typed_static_rhs_outlives_type_error_allocation() {
    let (status, stdout, stderr) = run_stdin(
        r#"<?php
error_reporting(0);
trait TypedSlot { public static self $value; }
try {
    TypedSlot::$value = new stdClass;
} catch (TypeError $error) {
    echo "error#", spl_object_id($error), "\n";
}
$replacement = new stdClass;
echo "replacement#", spl_object_id($replacement), "\n";
"#,
    );

    assert_eq!(status, 0);
    assert_eq!(stdout, "error#2\nreplacement#1\n");
    assert_eq!(stderr, "");
}
