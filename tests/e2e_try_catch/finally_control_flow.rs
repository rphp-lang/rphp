#[test]
fn test_try_finally_normal_flow() {
    // Finally runs even when no exception
    assert_eq!(
        run_php(
            r#"<?php
try {
    echo "try";
} finally {
    echo " finally";
}
"#
        ),
        "try finally"
    );
}

#[test]
fn test_try_catch_finally_catch_flow() {
    // Finally runs after catch
    assert_eq!(
        run_php(
            r#"<?php
try {
    throw new Exception("err");
} catch (Exception $e) {
    echo "caught";
} finally {
    echo " finally";
}
"#
        ),
        "caught finally"
    );
}

#[test]
fn test_try_finally_on_throw() {
    // Finally runs even when throw occurs and there's no catch
    let err = run_php_expect_error(
        r#"<?php
try {
    echo "before ";
    throw new Exception("boom");
} finally {
    echo "finally";
}
"#,
    );
    // Finally should have run (producing output) but the exception is still uncaught
    match err {
        rphp::vm::execute::VmError::Fatal(msg) => {
            assert!(
                msg.contains("Uncaught"),
                "Expected uncaught exception, got: {}",
                msg
            );
        }
        other => panic!("Expected Fatal, got: {:?}", other),
    }
}

#[test]
fn test_try_catch_finally_no_throw() {
    // Normal flow: try + finally, no exception
    assert_eq!(
        run_php(
            r#"<?php
try {
    echo "ok";
} catch (Exception $e) {
    echo "caught";
} finally {
    echo " done";
}
"#
        ),
        "ok done"
    );
}

#[test]
fn test_return_inside_try_runs_finally() {
    // PHP: finally runs even when return is in try block
    assert_eq!(
        run_php(
            r#"<?php
function f() {
    try {
        return "T";
    } finally {
        echo "F";
    }
}
echo f();
"#
        ),
        "FT"
    );
}

#[test]
fn test_return_inside_catch_runs_finally() {
    assert_eq!(
        run_php(
            r#"<?php
function f() {
    try {
        throw new Exception("err");
    } catch (Exception $e) {
        return "C";
    } finally {
        echo "F";
    }
}
echo f();
"#
        ),
        "FC"
    );
}

#[test]
fn test_nested_try_finally_exception_propagates() {
    // Inner finally runs, then outer catch handles the exception
    assert_eq!(
        run_php(
            r#"<?php
try {
    try {
        throw new Exception("boom");
    } finally {
        echo "F ";
    }
} catch (Exception $e) {
    echo "caught: " . $e->getMessage();
}
"#
        ),
        "F caught: boom"
    );
}

#[test]
fn test_throw_undef_in_try_finally_is_fatal() {
    // `throw $x` where $x is undefined → fatal: not a Throwable
    let err = run_php_expect_error(
        r#"<?php
function f() {
    try {
        throw $x;
    } finally {
        echo "F";
    }
}
f();
"#,
    );
    match err {
        rphp::vm::execute::VmError::Fatal(msg) => {
            assert!(
                msg.starts_with("Uncaught Error: Can only throw objects"),
                "Expected PHP throw validation error, got: {}",
                msg
            );
        }
        other => panic!("Expected Fatal, got: {:?}", other),
    }
}

#[test]
fn test_nested_finally_return_not_lost() {
    assert_eq!(
        run_php(
            r#"<?php
function inner() {
    try {
        return "i";
    } finally {
    }
}
function outer() {
    try {
        return inner() . "o";
    } finally {
        echo "F";
    }
}
echo outer();
"#
        ),
        "Fio"
    );
}

#[test]
fn test_return_in_finally_suppresses_exception() {
    // PHP semantics: return inside finally suppresses any pending exception.
    assert_eq!(
        run_php(
            r#"<?php
function f() {
    try {
        throw new Exception("err");
    } finally {
        return 1;
    }
}

function g() {
    try {
        return "Z";
    } finally {
    }
}
echo "F" . f() . "G" . g();
"#
        ),
        "F1GZ"
    );
}

#[test]
fn escaping_finally_exception_appends_displaced_previous_chain() {
    assert_eq!(
        run_php(
            r#"<?php
$explicit = new Exception('explicit');
try {
    try { throw new Exception('old'); }
    finally { throw new Exception('new', 0, $explicit); }
} catch (Throwable $error) {
    for (; $error; $error = $error->getPrevious()) echo $error->getMessage(), "\n";
}
try {
    try { throw new Exception('kept'); }
    finally {
        try { throw new Exception('caught'); }
        catch (Throwable $error) { var_dump($error->getPrevious()); }
    }
} catch (Throwable $error) { echo $error->getMessage(), "\n"; }
"#
        ),
        "new\nexplicit\nold\nNULL\nkept\n"
    );
}
