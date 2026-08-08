#[test]
fn test_try_catch_basic() {
    assert_eq!(
        run_php(
            r#"<?php
try {
    throw new Exception("error!");
} catch (Exception $e) {
    echo "caught: " . $e->getMessage();
}
"#
        ),
        "caught: error!"
    );
}

#[test]
fn test_try_catch_no_throw() {
    assert_eq!(
        run_php(
            r#"<?php
try {
    echo "ok";
} catch (Exception $e) {
    echo "caught";
}
"#
        ),
        "ok"
    );
}

#[test]
fn test_try_catch_code_after() {
    assert_eq!(
        run_php(
            r#"<?php
try {
    throw new Exception("err");
} catch (Exception $e) {
    echo "caught";
}
echo " done";
"#
        ),
        "caught done"
    );
}

#[test]
fn test_try_catch_throw_skips_rest() {
    assert_eq!(
        run_php(
            r#"<?php
try {
    echo "before";
    throw new Exception("err");
    echo "after";
} catch (Exception $e) {
    echo " caught";
}
"#
        ),
        "before caught"
    );
}

#[test]
fn test_try_catch_exception_value() {
    assert_eq!(
        run_php(
            r#"<?php
try {
    throw new Exception("specific error");
} catch (Exception $e) {
    echo $e->getMessage();
}
"#
        ),
        "specific error"
    );
}

#[test]
fn test_try_catch_exception_with_getmessage() {
    assert_eq!(
        run_php(
            r#"<?php
try {
    throw new Exception("hello world");
} catch (Exception $e) {
    echo $e->getMessage();
}
"#
        ),
        "hello world"
    );
}

#[test]
fn test_uncaught_exception_is_fatal() {
    let err = run_php_expect_error(r#"<?php throw new Exception("boom");"#);
    match err {
        rphp::vm::execute::VmError::Fatal(msg) => {
            assert!(
                msg.contains("Uncaught"),
                "Expected uncaught exception, got: {}",
                msg
            );
            assert!(
                msg.contains("boom"),
                "Expected message in error, got: {}",
                msg
            );
        }
        other => panic!("Expected Fatal, got: {:?}", other),
    }
}

#[test]
fn test_try_catch_in_function() {
    assert_eq!(
        run_php(
            r#"<?php
function risky() {
    throw new Exception("oops");
}
try {
    risky();
} catch (Exception $e) {
    echo "caught: " . $e->getMessage();
}
"#
        ),
        "caught: oops"
    );
}

#[test]
fn test_nested_try_catch() {
    assert_eq!(
        run_php(
            r#"<?php
try {
    try {
        throw new Exception("inner");
    } catch (Exception $e) {
        echo "inner: " . $e->getMessage();
    }
    echo " outer ok";
} catch (Exception $e2) {
    echo "outer: " . $e2->getMessage();
}
"#
        ),
        "inner: inner outer ok"
    );
}
