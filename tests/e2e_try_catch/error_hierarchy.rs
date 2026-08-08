// ============================================================
// Error vs Exception hierarchy tests
// ============================================================

#[test]
fn test_catch_error_catches_undefined_function() {
    // PHP 8: undefined function throws Error, catchable by catch(Error)
    assert_eq!(
        run_php(
            r#"<?php
try {
    nonexistent();
} catch (Error $e) {
    echo "caught: " . $e->getMessage();
}
"#
        ),
        "caught: Call to undefined function nonexistent()"
    );
}

#[test]
fn test_catch_exception_does_not_catch_error() {
    // PHP 8: catch(Exception) does NOT catch Error
    let err = run_php_expect_error(
        r#"<?php
try {
    nonexistent();
} catch (Exception $e) {
    echo "caught";
}
"#,
    );
    match err {
        rphp::vm::execute::VmError::Fatal(msg) => {
            assert!(
                msg.contains("Uncaught Error"),
                "Expected Uncaught Error, got: {}",
                msg
            );
        }
        other => panic!("Expected Fatal, got: {:?}", other),
    }
}

#[test]
fn test_catch_throwable_catches_both() {
    // catch(Throwable) catches both Error and Exception
    assert_eq!(
        run_php(
            r#"<?php
try {
    nonexistent();
} catch (Throwable $e) {
    echo "caught error";
}
try {
    throw new Exception("ex");
} catch (Throwable $e) {
    echo " caught exception";
}
"#
        ),
        "caught error caught exception"
    );
}

#[test]
fn test_catch_error_does_not_catch_exception() {
    // catch(Error) does NOT catch Exception
    let err = run_php_expect_error(
        r#"<?php
try {
    throw new Exception("test");
} catch (Error $e) {
    echo "caught";
}
"#,
    );
    match err {
        rphp::vm::execute::VmError::Fatal(msg) => {
            assert!(
                msg.contains("Uncaught Exception"),
                "Expected Uncaught Exception, got: {}",
                msg
            );
        }
        other => panic!("Expected Fatal, got: {:?}", other),
    }
}

#[test]
fn test_catch_typeerror() {
    // TypeError extends Error, catchable by catch(TypeError) and catch(Error)
    assert_eq!(
        run_php(
            r#"<?php
try {
    array_map("nonexistent", [1]);
} catch (TypeError $e) {
    echo "TypeError";
}
"#
        ),
        "TypeError"
    );
}

#[test]
fn test_catch_error_catches_typeerror() {
    // TypeError extends Error, so catch(Error) catches it too
    assert_eq!(
        run_php(
            r#"<?php
try {
    array_map("nonexistent", [1]);
} catch (Error $e) {
    echo "Error";
}
"#
        ),
        "Error"
    );
}
